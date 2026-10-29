//! What every thread is built from: the database, the model, the config and the paths.
//! The runtime builds chat threads with it, and agents build the threads of the agents
//! they ask. It also carries messages between agents, starting an agent's runner when
//! a message finds it stopped.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use tracing::warn;

use super::{
    inbox::Input,
    mailboxes::Mailboxes,
    model_choice::ModelChoice,
    thread_runner::{ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup},
};
use crate::{
    BoxFuture,
    graph::{AGENT_FILE, Graph, Node},
    live_config::LiveConfig,
    paths::Paths,
    state::{State, Thread, TurnStatus, estimate_tokens},
    tools::{
        AskAgent, GENERAL_AGENT, MAX_AGENT_DEPTH, MessageParent, ReadHistory, ToolRegistry,
        ToolScope,
    },
};

/// Agents at work at once, across every thread.
const MAX_AGENTS_AT_WORK: usize = 8;
/// Messages from agents a chat takes between two of the user's.
const MAX_AGENT_MESSAGES: u64 = 30;

#[derive(Clone)]
pub struct Launcher {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub paths: Paths,
    pub mailboxes: Arc<Mailboxes>,
}

impl Launcher {
    /// `agent` is the agent the thread works as, `None` for a chat or the general agent;
    /// `depth` counts the agents above it.
    pub fn setup(
        &self,
        thread_id: &str,
        cwd: &Path,
        workspace: &Path,
        agent: Option<&Node>,
        depth: usize,
    ) -> Result<ThreadSetup> {
        Ok(ThreadSetup {
            state: Arc::clone(&self.state),
            model: Arc::clone(&self.model),
            config: Arc::clone(&self.config),
            thread_id: thread_id.to_owned(),
            tools: self.tools(thread_id, agent, depth),
            scope: ToolScope::new(cwd, workspace)?,
            compact_prompt: self.paths.compact_prompt(),
            depth,
            mailboxes: Arc::clone(&self.mailboxes),
            ends_when_idle: false,
        })
    }

    /// The built-in tools, the thread's own history, other agents while the depth
    /// allows, and for an agent a way to write to the thread that asked it. An agent
    /// with a `tools` list keeps only those, and that way back.
    pub fn tools(&self, thread_id: &str, agent: Option<&Node>, depth: usize) -> ToolRegistry {
        let mut tools = ToolRegistry::builtin(&self.paths.skills_dir());
        let history = ReadHistory::new(Arc::clone(&self.state), thread_id.to_owned());
        tools.add(Box::new(history));
        if depth < MAX_AGENT_DEPTH {
            let asker = agent.map(|agent| agent.name.clone());
            let ask = AskAgent::new(self.clone(), thread_id.to_owned(), asker);
            tools.add(Box::new(ask));
        }
        if let Some(allowed) = agent.and_then(|agent| agent.tools.as_ref()) {
            tools.keep(allowed);
        }
        if depth > 0 {
            tools.add(Box::new(MessageParent::new(
                self.clone(),
                thread_id.to_owned(),
            )));
        }
        tools
    }

    /// Stores a message from one agent's thread to another and makes sure it is read:
    /// a running thread is woken, a stopped agent thread is started. A chat that is
    /// not open takes the message in when it is opened again. Agents are bounded: so
    /// many at work at once, and so many messages to a chat between two of the user's,
    /// so agents cannot keep each other busy without end. Boxed, because a started
    /// agent may deliver in turn.
    pub fn deliver<'a>(
        &'a self,
        to: &'a str,
        from: &'a str,
        content: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let limit = self.config.current().limits().max_input_tokens;
            let tokens = estimate_tokens(content);
            ensure!(
                tokens <= limit,
                "the message is about {tokens} tokens, past the input limit of {limit}"
            );
            // Held until the message is stored and its reader is sure to see it.
            let mut live = self.mailboxes.lock().await;
            let running = live.get(to).filter(|mailbox| mailbox.is_live()).cloned();
            let thread = self
                .state
                .load_thread(to)
                .await?
                .with_context(|| format!("thread {to} does not exist"))?;
            match &thread.parent_thread_id {
                None => {
                    let received = self.state.agent_messages_since_client(to).await?;
                    ensure!(
                        received < MAX_AGENT_MESSAGES,
                        "the chat has had {MAX_AGENT_MESSAGES} agent messages since the user \
                         last wrote; it takes no more until then"
                    );
                }
                Some(_) if running.is_none() && !self.mailboxes.is_closed() => {
                    let at_work = live.values().filter(|mailbox| mailbox.agent).count();
                    ensure!(
                        at_work < MAX_AGENTS_AT_WORK,
                        "{MAX_AGENTS_AT_WORK} agents are at work already; wait for one to finish"
                    );
                }
                Some(_) => {}
            }
            self.state.add_pending_from(to, content, Some(from)).await?;
            if running.is_some_and(|mailbox| mailbox.send(Input::Sent)) {
                return Ok(());
            }
            // After shutdown, or for a chat that is not open, the message waits stored.
            let Some(parent) = thread.parent_thread_id.clone() else {
                return Ok(());
            };
            if self.mailboxes.is_closed() {
                return Ok(());
            }
            let (runner, input, output) = self.agent_runner(&thread, cwd).await?;
            live.insert(to.to_owned(), runner.mailbox(&input));
            drop(live);
            tokio::spawn(async move {
                // Kept for as long as the runner, which ends on its own when idle.
                let _input = input;
                runner.run().await;
            });
            let reports: Pin<Box<dyn Future<Output = ()> + Send>> =
                Box::pin(self.clone().report(output, thread, parent, cwd.to_owned()));
            tokio::spawn(reports);
            Ok(())
        })
    }

    async fn agent_runner(
        &self,
        thread: &Thread,
        cwd: &Path,
    ) -> Result<(ThreadRunner, ThreadInput, ThreadOutput)> {
        let depth = self.state.thread_depth(&thread.id).await?;
        let name = thread.agent.as_deref().unwrap_or(GENERAL_AGENT);
        let graph = Graph::load(&self.paths.agents_dir(), AGENT_FILE);
        let node = graph.get(name);
        let workspace = self.paths.thread_workspace(&thread.id)?;
        let mut setup = self.setup(&thread.id, cwd, &workspace, node, depth)?;
        setup.ends_when_idle = true;
        Ok(ThreadRunner::new(setup))
    }

    /// Passes an agent's events on to the client of the thread that asked it, and its
    /// answer to that thread when a turn ends. A turn the user stopped reports nothing,
    /// so a stop does not wake the asking agent again.
    async fn report(self, mut output: ThreadOutput, thread: Thread, parent: String, cwd: PathBuf) {
        while let Some(event) = output.next_event().await {
            let report = match &event {
                ThreadEvent::TurnFinished(turn) => match turn.status {
                    TurnStatus::Completed => {
                        let answer = self
                            .answer(&thread.id, turn.response_message_id.as_deref())
                            .await;
                        Some(format!("finished:\n{answer}"))
                    }
                    TurnStatus::Failed => Some(format!(
                        "failed: {}",
                        turn.error_message.as_deref().unwrap_or("no reason given")
                    )),
                    _ => None,
                },
                // The runner itself stopped, such as on a database error.
                ThreadEvent::Failed(error) => Some(format!("failed: {error}")),
                _ => None,
            };
            if let Some(report) = report
                && let Err(error) = self.deliver(&parent, &thread.id, &report, &cwd).await
            {
                warn!("cannot report agent thread {}: {error:#}", thread.id);
            }
            if let Some(mailbox) = self.mailboxes.get(&parent).await {
                mailbox.emit(ThreadEvent::Subagent {
                    thread_id: thread.id.clone(),
                    event: Box::new(event),
                });
            }
        }
    }

    async fn answer(&self, thread_id: &str, message_id: Option<&str>) -> String {
        let Some(message_id) = message_id else {
            return "(no answer)".into();
        };
        let messages = self
            .state
            .list_messages(thread_id)
            .await
            .unwrap_or_default();
        messages
            .into_iter()
            .find(|message| message.message_id == message_id)
            .map_or_else(|| "(no answer)".into(), |message| message.content)
    }
}
