//! Agents talking to agents. `ask_agent` hands a task to another agent, which works in a
//! thread of its own under the asking one, with its own system prompt, context and
//! tools, in the background: the call returns at once. Asking the same thread again
//! sends it more while it works. The agent writes back with `message_parent`, and its
//! final answer comes back on its own. Every message goes through the database, so
//! none is lost, and arrives between the reader's steps.
//!
//! A chat may ask the entry agents; an agent may ask the agents it links to. Any thread
//! may ask the general agent, which works like a chat: MAIN.md and every tool, so
//! delegating needs no AGENT.md. Depth is bounded, so agents cannot hand work down forever.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Progress, Tool, ToolContext};
use crate::{
    BoxFuture,
    graph::{AGENT_FILE, Graph, Node},
    runtime::{ThreadEvent, launcher::Launcher},
    thread_start::{read_prompt, start_agent_thread},
};

/// A chat is at depth 0; an agent three levels down can no longer ask another.
pub const MAX_AGENT_DEPTH: usize = 3;

/// The built-in agent, asked when none is named. An `agents/general` folder replaces it.
pub const GENERAL_AGENT: &str = "general";
/// Agent threads one thread may open; asking an earlier one again is not limited.
const MAX_AGENTS_PER_THREAD: usize = 16;
const GENERAL_DESCRIPTION: &str = "works like this chat, with MAIN.md and every tool; \
for research, reading code and self-contained parts of a task";

pub struct AskAgent {
    launcher: Launcher,
    /// The asking thread, which becomes the parent of the agent's thread.
    thread_id: String,
    /// The agent the asking thread works as; `None` for a chat.
    asker: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskAgentArguments {
    agent: Option<String>,
    task: String,
    thread: Option<String>,
}

impl AskAgent {
    pub fn new(launcher: Launcher, thread_id: String, asker: Option<String>) -> Self {
        Self {
            launcher,
            thread_id,
            asker,
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

    async fn ask(
        &self,
        arguments: AskAgentArguments,
        cwd: &Path,
        progress: Option<Progress<'_>>,
    ) -> Result<String> {
        ensure!(!arguments.task.trim().is_empty(), "task is empty");
        let name = arguments.agent.as_deref().unwrap_or(GENERAL_AGENT);
        // `None` is the built-in general agent.
        let agent = self.reachable().into_iter().find(|node| node.name == name);
        ensure!(
            agent.is_some() || name == GENERAL_AGENT,
            "no agent {name} to ask from here"
        );
        let thread_id = match &arguments.thread {
            Some(thread_id) => {
                self.check_earlier_thread(thread_id, name).await?;
                thread_id.clone()
            }
            None => {
                let opened = self.launcher.state.agent_threads(&self.thread_id).await?;
                ensure!(
                    opened.len() < MAX_AGENTS_PER_THREAD,
                    "this thread has opened {MAX_AGENTS_PER_THREAD} agent threads; continue \
                     one of them with `thread` instead"
                );
                let paths = &self.launcher.paths;
                let prompt = match &agent {
                    Some(agent) => Some(agent.instructions()?),
                    None => read_prompt(&paths.main_prompt())?,
                };
                let state = &self.launcher.state;
                let thread =
                    start_agent_thread(state, paths, &self.thread_id, name, prompt).await?;
                thread.thread_id
            }
        };
        if let Some(progress) = progress {
            progress.emit(ThreadEvent::SubagentStarted {
                call_id: progress.call_id.to_owned(),
                thread_id: thread_id.clone(),
                agent: name.to_owned(),
                task: arguments.task.clone(),
            });
        }
        self.launcher
            .deliver(&thread_id, &self.thread_id, &arguments.task, cwd)
            .await?;
        Ok(format!(
            "thread: {thread_id}\nAgent {name} works on it in the background. What it writes \
             and its final answer will come to you as messages from this thread. To send it \
             more, call ask_agent with this thread."
        ))
    }

    /// A thread this thread opened with the same agent before.
    async fn check_earlier_thread(&self, thread_id: &str, agent: &str) -> Result<()> {
        let thread = self.launcher.state.load_thread(thread_id).await?;
        let belongs = thread.is_some_and(|thread| {
            thread.parent_thread_id.as_deref() == Some(self.thread_id.as_str())
                && thread.agent.as_deref() == Some(agent)
        });
        ensure!(
            belongs,
            "thread {thread_id} is not a conversation with {agent} from here"
        );
        Ok(())
    }
}

impl Tool for AskAgent {
    fn name(&self) -> &'static str {
        "ask_agent"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        let reachable = self.reachable();
        let mut agents: Vec<String> = reachable
            .iter()
            .map(|node| format!("- {}: {}", node.name, node.description))
            .collect();
        if !reachable.iter().any(|node| node.name == GENERAL_AGENT) {
            agents.insert(0, format!("- {GENERAL_AGENT}: {GENERAL_DESCRIPTION}"));
        }
        format!(
            "Give a task to a subagent, or send more to one at work. It works in its own \
             thread with its own context and tools, in the background, so a long search or \
             a self-contained part of the work does not fill this conversation, and several \
             can work at once. It sees nothing of this conversation, so describe the task \
             fully. Leave out `agent` for {GENERAL_AGENT}. The call returns the agent's \
             thread at once; what the agent writes and its final answer arrive later as \
             messages from that thread. Pass the thread to write to the same agent again; \
             end your turn when there is nothing else to do until it answers.\nAgents:\n{}",
            agents.join("\n")
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Agent name, as listed; general when left out."
                },
                "task": { "type": "string", "description": "Everything the agent needs." },
                "thread": {
                    "type": "string",
                    "description": "A thread the agent answered from before, to continue it."
                }
            },
            "required": ["task"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: AskAgentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.ask(arguments, &cwd, context.progress).await
        })
    }
}

/// `message_parent`: an agent writes to the thread that asked it, while it works.
pub struct MessageParent {
    launcher: Launcher,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageParentArguments {
    text: String,
}

impl MessageParent {
    pub fn new(launcher: Launcher, thread_id: String) -> Self {
        Self {
            launcher,
            thread_id,
        }
    }

    async fn send(&self, text: &str, cwd: &Path) -> Result<String> {
        ensure!(!text.trim().is_empty(), "text is empty");
        let thread = self.launcher.state.load_thread(&self.thread_id).await?;
        let parent = thread
            .and_then(|thread| thread.parent_thread_id)
            .context("this thread has no thread above it")?;
        self.launcher
            .deliver(&parent, &self.thread_id, text, cwd)
            .await?;
        Ok("sent".into())
    }
}

impl Tool for MessageParent {
    fn name(&self) -> &'static str {
        "message_parent"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Write to the agent that gave you your task, while you work: a question, a finding \
         it should know now, how far you are. It reads the message between its steps. Your \
         final answer reaches it on its own, so do not send that too."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The message." }
            },
            "required": ["text"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: MessageParentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.send(&arguments.text, &cwd).await
        })
    }
}
