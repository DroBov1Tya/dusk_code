//! Messages between agents: stored for the thread they are for, which is woken if it runs
//! or started if it is a stopped agent; an agent's events and answer go back to its asker.
//! An `impl Launcher` block, since every thread is built by the launcher.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

use anyhow::{Context, Result, ensure};
use tracing::warn;

use super::Input;
use crate::{
    BoxFuture,
    catalog::agents::GENERAL_AGENT,
    runtime::{Launcher, ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner},
    state::{Thread, TurnStatus, estimate_tokens},
};

/// Messages from agents a chat takes between two of the user's.
const MAX_AGENT_MESSAGES: u64 = 30;

impl Launcher {
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
                .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
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

    /// Agents at work and agents waiting for a slot. A new agent is refused while they
    /// reach the limit: past it, it would only wait, and the asker would believe it works.
    pub async fn admit_agent(&self) -> Result<tokio::sync::MutexGuard<'_, ()>> {
        let gate = self.mailboxes.new_agents.lock().await;
        let live: Vec<String> = self
            .mailboxes
            .lock()
            .await
            .iter()
            .filter(|(_, mailbox)| mailbox.agent && mailbox.is_live())
            .map(|(id, _)| id.clone())
            .collect();
        let mut busy = live.len();
        for thread_id in self.state.threads_with_pending().await? {
            if live.contains(&thread_id) {
                continue;
            }
            let thread = self.state.load_thread(&thread_id).await?;
            if thread.is_some_and(|thread| thread.parent_thread_id.is_some()) {
                busy += 1;
            }
        }
        let limit = self.config.current().limits().max_agents_at_work;
        ensure!(
            busy < limit,
            "{busy} agents are at work or waiting, the limit is {limit}; wait for one to \
             finish (its answer arrives as a message), give the task to one of them with \
             `thread`, or do it yourself"
        );
        Ok(gate)
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
            let root = self.state.root_thread(&thread.id).await?;
            let (runner, input, output) = self.agent_runner(thread, cwd).await?;
            live.insert(thread.id.clone(), runner.mailbox(&input));
            self.mailboxes.begin_agent_report(&thread.id).await;
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
                        .report(output, thread.clone(), parent, root, cwd.to_owned()),
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
        let workspace =
            crate::runtime::thread::start::workspace(&self.state, &self.paths, &thread.id).await?;
        let mut setup = self.setup(&thread.id, cwd, &workspace, identity.as_ref(), depth)?;
        setup.ends_when_idle = true;
        Ok(ThreadRunner::new(setup))
    }

    /// Passes an agent's events on to the client of the thread that asked it, and its
    /// answer to that thread when a turn ends, however it ends: the asker must never
    /// wait for an agent that is gone.
    async fn report(
        self,
        mut output: ThreadOutput,
        thread: Thread,
        parent: String,
        root: String,
        cwd: PathBuf,
    ) {
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
                    TurnStatus::Interrupted => {
                        let answer = match turn.response_message_id.as_deref() {
                            Some(id) => self.answer(&thread.id, Some(id)).await,
                            None => "(no answer yet)".into(),
                        };
                        let stopped = format!("was stopped before it finished; so far:\n{answer}");
                        if let Err(error) = self.tell_stopped(&parent, &thread.id, &stopped).await {
                            warn!("cannot report agent thread {}: {error:#}", thread.id);
                        }
                        None
                    }
                    TurnStatus::Running => None,
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
        // The report stream was drained before clearing the dependency. The wake also
        // covers an interrupted turn, which deliberately has no result to deliver.
        let live = self.mailboxes.lock().await;
        self.mailboxes.end_agent_report(&thread.id).await;
        if let Some(mailbox) = live.get(&root) {
            mailbox.send(Input::DependencyFinished);
        }
    }

    /// A stop must reach the asker without restarting work the user stopped: the report
    /// waits stored, and is read at once only by a thread at work right now. An agent
    /// asker that is not at work was stopped with it and is not told; the chat always is,
    /// with its next request.
    pub(crate) async fn tell_stopped(&self, parent: &str, from: &str, report: &str) -> Result<()> {
        let live = self
            .mailboxes
            .get(parent)
            .await
            .filter(|mailbox| mailbox.is_live());
        let is_chat = self
            .state
            .load_thread(parent)
            .await?
            .is_some_and(|thread| thread.parent_thread_id.is_none());
        if live.is_none() && !is_chat {
            return Ok(());
        }
        self.state
            .add_pending_from(parent, report, Some(from))
            .await?;
        if let Some(mailbox) = live {
            mailbox.send(Input::Sent);
        }
        Ok(())
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
