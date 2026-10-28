//! `ask_agent`: one agent hands a task to another. The other agent works in a thread of
//! its own, under the asking thread, with its own system prompt, context and tools, and
//! its final answer comes back as the tool result. Asking the same thread again goes on
//! with the same conversation.
//!
//! A chat may ask the entry agents; an agent may ask the agents it links to. Depth is
//! bounded, so agents cannot hand work down forever.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    launcher::Launcher,
    thread_runner::{ThreadEvent, ThreadRunner},
};
use crate::{
    BoxFuture,
    graph::{AGENT_FILE, Graph, Node},
    state::TurnStatus,
    thread_start::start_agent_thread,
    tools::{Tool, ToolContext},
};

/// A chat is at depth 0; an agent three levels down can no longer ask another.
pub const MAX_AGENT_DEPTH: usize = 3;

pub struct AskAgent {
    launcher: Launcher,
    /// The asking thread, which becomes the parent of the agent's thread.
    thread_id: String,
    /// The agent the asking thread works as; `None` for a chat.
    asker: Option<String>,
    depth: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskAgentArguments {
    agent: String,
    task: String,
    thread: Option<String>,
}

impl AskAgent {
    pub fn new(launcher: Launcher, thread_id: String, asker: Option<String>, depth: usize) -> Self {
        Self {
            launcher,
            thread_id,
            asker,
            depth,
        }
    }

    /// The agents this thread may ask: the entries for a chat, the links for an agent.
    fn reachable(&self) -> Vec<Node> {
        let graph = Graph::load(&self.launcher.paths.agents_dir(), AGENT_FILE);
        match &self.asker {
            None => graph.entries().cloned().collect(),
            Some(asker) => match graph.get(asker) {
                Some(node) => graph.links(node).cloned().collect(),
                None => Vec::new(),
            },
        }
    }

    async fn ask(&self, arguments: AskAgentArguments, cwd: &Path) -> Result<String> {
        ensure!(!arguments.task.trim().is_empty(), "task is empty");
        let agent = self
            .reachable()
            .into_iter()
            .find(|node| node.name == arguments.agent)
            .with_context(|| format!("no agent {} to ask from here", arguments.agent))?;
        let (thread_id, workspace) = match &arguments.thread {
            Some(thread_id) => self.earlier_thread(thread_id, &agent.name).await?,
            None => {
                let paths = &self.launcher.paths;
                let thread =
                    start_agent_thread(&self.launcher.state, paths, &self.thread_id, &agent)
                        .await?;
                (thread.thread_id, thread.workspace)
            }
        };
        let setup =
            self.launcher
                .setup(&thread_id, cwd, &workspace, Some(&agent), self.depth + 1)?;
        let (runner, input, mut output) = ThreadRunner::new(setup);
        input.send(arguments.task).await?;
        // The thread ends once its queue is done: this one message.
        drop(input);
        let last_turn = async {
            let mut finished = None;
            while let Some(event) = output.next_event().await {
                if let ThreadEvent::TurnFinished(turn) = event {
                    finished = Some(turn);
                }
            }
            finished
        };
        let ((), finished) = tokio::join!(runner.run(), last_turn);
        let turn = finished.context("the agent did not take up the task")?;
        let answer = match &turn.response_message_id {
            Some(id) => self.answer(&thread_id, id).await?,
            None => String::new(),
        };
        let thread = format!("thread: {thread_id}");
        match turn.status {
            TurnStatus::Completed => Ok(format!("{thread}\n{answer}")),
            TurnStatus::Interrupted if !answer.is_empty() => Ok(format!(
                "{thread}\nstopped before the end; what it wrote:\n{answer}"
            )),
            _ => {
                let error = turn.error_message.unwrap_or_else(|| "stopped".into());
                bail!("agent {} failed ({thread}): {error}", agent.name)
            }
        }
    }

    /// A thread this thread opened with the same agent before.
    async fn earlier_thread(&self, thread_id: &str, agent: &str) -> Result<(String, PathBuf)> {
        let thread = self.launcher.state.load_thread(thread_id).await?;
        let belongs = thread.is_some_and(|thread| {
            thread.parent_thread_id.as_deref() == Some(self.thread_id.as_str())
                && thread.agent.as_deref() == Some(agent)
        });
        ensure!(
            belongs,
            "thread {thread_id} is not a conversation with {agent} from here"
        );
        let workspace = self.launcher.paths.thread_workspace(thread_id)?;
        Ok((thread_id.to_owned(), workspace))
    }

    async fn answer(&self, thread_id: &str, message_id: &str) -> Result<String> {
        let messages = self.launcher.state.list_messages(thread_id).await?;
        let answer = messages
            .into_iter()
            .find(|message| message.message_id == message_id);
        Ok(answer.map(|message| message.content).unwrap_or_default())
    }
}

impl Tool for AskAgent {
    fn name(&self) -> &'static str {
        "ask_agent"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        let agents: Vec<String> = self
            .reachable()
            .iter()
            .map(|node| format!("- {}: {}", node.name, node.description))
            .collect();
        format!(
            "Give a task to another agent. It works in its own thread with its own context \
             and tools, and its final answer comes back here. It sees nothing of this \
             conversation, so describe the task fully. The answer starts with the agent's \
             thread; pass it as `thread` to go on with the same agent later.\nAgents:\n{}",
            agents.join("\n")
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": { "type": "string", "description": "Agent name, as listed." },
                "task": { "type": "string", "description": "Everything the agent needs." },
                "thread": {
                    "type": "string",
                    "description": "A thread the agent answered from before, to continue it."
                }
            },
            "required": ["agent", "task"],
            "additionalProperties": false
        })
    }

    fn available(&self, _context: &ToolContext<'_>) -> bool {
        !self.reachable().is_empty()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: AskAgentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.ask(arguments, &cwd).await
        })
    }
}
