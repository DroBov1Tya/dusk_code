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
    ThreadEvent, ThreadInput, ThreadLock, ThreadOutput, ThreadRunner, ThreadSetup,
    model_choice::ModelChoice,
};
use crate::runtime::agent::{Input, Mailboxes};
use crate::{
    BoxFuture,
    agents::{Agents, GENERAL_AGENT, MAX_AGENT_DEPTH},
    artifacts::ArtifactStore,
    live_config::LiveConfig,
    mcp::McpServers,
    paths::Paths,
    state::{Identity, State, Thread, TurnStatus, estimate_tokens},
    tools::{
        AskAgent, AskUser, DisplayImage, McpTool, MessageParent, ReadHistory, Remember, Tasks,
        ToolRegistry, ToolScope,
    },
};

/// Messages from agents a chat takes between two of the user's.
const MAX_AGENT_MESSAGES: u64 = 30;

#[derive(Clone)]
pub struct Launcher {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub paths: Paths,
    pub mailboxes: Arc<Mailboxes>,
    pub mcp: Arc<McpServers>,
    pub agents: Arc<Agents>,
}

impl Launcher {
    /// `agent` is the agent the thread works as, `None` for a chat or the general agent;
    /// `depth` counts the agents above it.
    pub fn setup(
        &self,
        thread_id: &str,
        cwd: &Path,
        workspace: &Path,
        agent: Option<&Identity>,
        depth: usize,
    ) -> Result<ThreadSetup> {
        Ok(ThreadSetup {
            state: Arc::clone(&self.state),
            model: Arc::clone(&self.model),
            config: Arc::clone(&self.config),
            thread_id: thread_id.to_owned(),
            tools: self.tools(thread_id, agent, depth),
            scope: ToolScope::new(cwd, workspace)?
                .protect(self.paths.config_root())
                .share(&self.paths.skills_dir())
                .share(&self.paths.agents_dir()),
            compact_prompt: self.paths.compact_prompt(),
            fork_prompt: self.paths.fork_prompt(),
            modes_dir: self.paths.modes_dir(),
            depth,
            mailboxes: Arc::clone(&self.mailboxes),
            ends_when_idle: false,
            lock: ThreadLock::take(workspace)?,
        })
    }

    /// The built-in tools, the tools of MCP servers that are ready, the thread's own
    /// history, other agents while the depth allows, and for an agent a way to write to
    /// the thread that asked it. An agent
    /// with a `tools` list keeps only those, and that way back.
    pub fn tools(&self, thread_id: &str, agent: Option<&Identity>, depth: usize) -> ToolRegistry {
        let mut tools = ToolRegistry::builtin(&self.paths.skills_dir());
        tools.chat = depth == 0;
        tools.set_depth(depth);
        for server in self.mcp.ready() {
            for tool in McpTool::all(&server) {
                tools.add(Box::new(tool));
            }
        }
        let history = ReadHistory::new(Arc::clone(&self.state), thread_id.to_owned());
        tools.add(Box::new(history));
        tools.add(Box::new(Tasks::new(
            Arc::clone(&self.state),
            thread_id.to_owned(),
        )));
        tools.add(Box::new(Remember::new(self.paths.memory_file())));
        if depth < MAX_AGENT_DEPTH {
            let asker = agent.map(|agent| agent.name.clone());
            let ask = AskAgent::new(self.clone(), thread_id.to_owned(), asker);
            tools.add(Box::new(ask));
        }
        if let Some(allowed) = agent.and_then(|agent| agent.tools.as_ref()) {
            tools.keep(allowed);
        }
        // Only a chat has the user to ask.
        if depth == 0 {
            tools.add(Box::new(AskUser));
            tools.add(Box::new(DisplayImage::new(ArtifactStore::new(
                self.paths.artifacts_dir(),
            ))));
        }
        if depth > 0 {
            tools.add(Box::new(MessageParent::new(
                self.clone(),
                thread_id.to_owned(),
            )));
        }
        tools
    }

    /// Stores a message from one agent's thread to another and wakes its reader: a running
    /// thread is woken, a stopped agent thread is started. A message is never lost to a
    /// limit: it waits stored. Agents are bounded, so many at work at once: past that, an
    /// agent starts when another one ends. A chat is not woken by more than so many agent
    /// messages between two of the user's, so agents cannot keep it busy without end; the
    /// rest waits for the user's next message. Boxed, because a started agent may deliver.
    pub fn deliver<'a>(
        &'a self,
        to: &'a str,
        from: &'a str,
        content: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let config = self.config.current();
            let limit = self
                .model
                .current()
                .map_or(config.limits().max_input_tokens, |choice| {
                    choice.input_limit(&config)
                });
            let tokens = estimate_tokens(content);
            ensure!(
                tokens <= limit,
                "the message is about {tokens} tokens, past the input limit of {limit}"
            );
            let thread = self
                .state
                .load_thread(to)
                .await?
                .with_context(|| format!("thread {to} does not exist"))?;
            let chat_is_full = thread.parent_thread_id.is_none()
                && self.state.agent_messages_since_client(to).await? >= MAX_AGENT_MESSAGES;
            self.state.add_pending_from(to, content, Some(from)).await?;
            if chat_is_full {
                return Ok(());
            }
            self.wake(&thread, cwd).await
        })
    }

    /// Makes sure a thread with stored messages reads them: a running one is woken, a
    /// stopped agent thread is started while fewer than the limit are at work. A chat that
    /// is not open reads them when it is opened again.
    fn wake<'a>(&'a self, thread: &'a Thread, cwd: &'a Path) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // Held until the reader is sure to see the message: a runner that leaves checks
            // for stored messages under the same lock.
            let mut live = self.mailboxes.lock().await;
            if let Some(mailbox) = live.get(&thread.id).filter(|mailbox| mailbox.is_live())
                && mailbox.send(Input::Sent)
            {
                return Ok(());
            }
            let Some(parent) = thread.parent_thread_id.clone() else {
                return Ok(());
            };
            let at_work = live.values().filter(|mailbox| mailbox.agent).count();
            if self.mailboxes.is_closed()
                || self.model.current().is_none()
                || at_work >= self.config.current().limits().max_agents_at_work
            {
                return Ok(());
            }
            let (runner, input, output) = self.agent_runner(thread, cwd).await?;
            live.insert(thread.id.clone(), runner.mailbox(&input));
            drop(live);
            let launcher = self.clone();
            let folder = cwd.to_owned();
            tokio::spawn(async move {
                // Kept for as long as the runner, which ends on its own when idle.
                let ended_well = runner.run().await;
                drop(input);
                // A slot is free: agents whose messages waited for one start now. Not
                // after a failure, which the next agent would likely meet as well.
                if ended_well {
                    launcher.wake_waiting(&folder).await;
                }
            });
            let reports: Pin<Box<dyn Future<Output = ()> + Send>> =
                Box::pin(
                    self.clone()
                        .report(output, thread.clone(), parent, cwd.to_owned()),
                );
            tokio::spawn(reports);
            Ok(())
        })
    }

    /// Wakes every agent thread that has stored messages and no runner.
    fn wake_waiting<'a>(&'a self, cwd: &'a Path) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let waiting = match self.state.threads_with_pending().await {
                Ok(waiting) => waiting,
                Err(error) => return warn!("cannot look for waiting agents: {error:#}"),
            };
            for thread_id in waiting {
                let thread = match self.state.load_thread(&thread_id).await {
                    Ok(Some(thread)) if thread.parent_thread_id.is_some() => thread,
                    _ => continue,
                };
                if let Err(error) = self.wake(&thread, cwd).await {
                    warn!("cannot start agent thread {thread_id}: {error:#}");
                }
            }
        })
    }

    async fn agent_runner(
        &self,
        thread: &Thread,
        cwd: &Path,
    ) -> Result<(ThreadRunner, ThreadInput, ThreadOutput)> {
        let depth = self.state.thread_depth(&thread.id).await?;
        let name = thread.agent.as_deref().unwrap_or(GENERAL_AGENT);
        let identity = self.agents.get(name);
        let workspace = self.paths.thread_workspace(&thread.id)?;
        let mut setup = self.setup(&thread.id, cwd, &workspace, identity.as_ref(), depth)?;
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
