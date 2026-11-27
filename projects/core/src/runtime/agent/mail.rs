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
    runtime::{
        Launcher, ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner, model_choice::Choice, team,
        thread::ChatModel, tiers,
    },
    state::{Thread, TurnStatus},
};

/// Messages from agents a chat takes between two of the user's.
pub(crate) const MAX_AGENT_MESSAGES: u64 = 30;

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
            self.deliver_as(to, from, None, content, cwd).await?;
            Ok(())
        })
    }

    /// The same under the sender's own `uid`, for a sender that cannot know whether it
    /// delivered before a stop: delivered twice, the message is stored once. Answers
    /// whether it was stored now. A message stored earlier that still waits has its
    /// reader woken again, since it may have found no free slot the first time.
    pub fn deliver_once<'a>(
        &'a self,
        to: &'a str,
        from: &'a str,
        uid: &'a str,
        content: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, bool> {
        self.deliver_as(to, from, Some(uid), content, cwd)
    }

    fn deliver_as<'a>(
        &'a self,
        to: &'a str,
        from: &'a str,
        uid: Option<&'a str>,
        content: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.model.ensure_fits(content, &self.config.current())?;
            let thread = self
                .state
                .load_thread(to)
                .await?
                .with_context(|| format!("thread {to} does not exist"))?;
            let chat_is_full = thread.parent_thread_id.is_none()
                && self.state.agent_messages_since_client(to).await? >= MAX_AGENT_MESSAGES;
            let stored = match uid {
                Some(uid) => {
                    let from = Some(from);
                    self.state.add_pending_once(to, uid, content, from).await?
                }
                None => {
                    self.state.add_pending_from(to, content, Some(from)).await?;
                    true
                }
            };
            if chat_is_full || !(stored || self.state.has_pending(to).await?) {
                return Ok(stored);
            }
            self.wake(&thread, cwd).await?;
            Ok(stored)
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
                || at_work >= self.config.current().limits().max_agents_at_work
            {
                return Ok(());
            }
            let root = self.state.root_thread(&thread.id).await?;
            // The agent answers on its chat's model, never on one chosen in another chat.
            let chat = match live.get(&root) {
                Some(mailbox) => ChatModel::of(mailbox, self),
                None => self.stored_chat_model(&root).await?,
            };
            if chat.choice.is_none() {
                return Ok(());
            }
            let (runner, input, output) = self.agent_runner(thread, cwd, &chat).await?;
            live.insert(thread.id.clone(), runner.mailbox(&input));
            self.mailboxes.begin_agent_report(&thread.id).await;
            // The chat shows it is at work while its agents are, whatever its own turn does.
            if let Some(chat) = live.get(&root) {
                chat.emit(ThreadEvent::AgentsWorking(true));
            }
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

    /// Wakes every agent thread that has stored messages and no runner, and every agent a
    /// stopped process left at work under a chat that is open again: it may have found
    /// no free slot when its chat woke it.
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
            if let Err(error) = self.wake_unfinished(None, cwd).await {
                warn!("cannot start agents left at work: {error:#}");
            }
        })
    }

    /// Wakes the agents a stopped process left at work: below `root`, or below any open
    /// chat. Each settles its own turn under its own lock as it starts.
    pub async fn wake_unfinished(&self, root: Option<&str>, cwd: &Path) -> Result<()> {
        for thread_id in self.state.unfinished_agents().await? {
            let chat = self.state.root_thread(&thread_id).await?;
            let open = match root {
                Some(root) => chat == root,
                None => self.mailboxes.get(&chat).await.is_some(),
            };
            let thread = match self.state.load_thread(&thread_id).await? {
                Some(thread) if open => thread,
                _ => continue,
            };
            if let Err(error) = self.wake(&thread, cwd).await {
                warn!("cannot start agent thread {thread_id}: {error:#}");
            }
        }
        Ok(())
    }

    async fn agent_runner(
        &self,
        thread: &Thread,
        cwd: &Path,
        chat: &ChatModel,
    ) -> Result<(ThreadRunner, ThreadInput, ThreadOutput)> {
        let depth = self.state.thread_depth(&thread.id).await?;
        let name = thread.agent.as_deref().unwrap_or(GENERAL_AGENT);
        let identity = self.agents.get(name);
        let workspace =
            crate::runtime::thread::start::workspace(&self.state, &self.paths, &thread.id).await?;
        let mut setup = self.setup(&thread.id, cwd, &workspace, identity.as_ref(), depth)?;
        setup.ends_when_idle = true;
        // A member of a team has what the core fixed for it, not what its role lists.
        let own = if team::is_member(thread) {
            team::limit_tools(&mut setup.tools, &self.config.current(), name);
            // A role may answer on a model of its own; without one it is the chat's.
            team::member_model(self, thread, chat.choice.as_ref()).await?
        } else {
            self.tier_model(&thread.id, chat.choice.as_ref()).await
        };
        setup.model = self.model.own(own.or_else(|| chat.choice.clone()));
        // It thinks from its chat's effort, whatever another chat chose.
        setup.model.set_effort(Some(chat.effort));
        Ok(ThreadRunner::new(setup))
    }

    /// The model a runner at work answers on. It keeps the model it was started with, so
    /// a tier picked meanwhile applies from its next wake.
    pub async fn running_model(&self, thread_id: &str) -> Option<String> {
        let mailbox = self.mailboxes.get(thread_id).await?;
        mailbox.is_live().then_some(())?;
        mailbox.context.model.current().map(|choice| choice.name())
    }

    /// The model the asker picked for an agent by tier, when that is not the chat's. One
    /// that cannot be asked now (its provider removed, switched off or down) leaves the
    /// agent on the chat's model rather than failing its work.
    async fn tier_model(&self, thread_id: &str, chat: Option<&Choice>) -> Option<Choice> {
        let name = match self.state.agent_model(thread_id).await {
            Ok(name) => name?,
            Err(error) => {
                warn!("thread {thread_id}: {error:#}");
                return None;
            }
        };
        let config = self.config.current();
        let chat = chat.map(Choice::name);
        if chat.as_deref() == Some(name.as_str()) || !tiers::connected(&config, &name) {
            return None;
        }
        match Choice::pick(&config, Some(&name)).await {
            Ok(choice) => Some(choice),
            Err(error) => {
                warn!(
                    "thread {thread_id}: {name} cannot be asked, the chat's model answers: {error:#}"
                );
                None
            }
        }
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
        // A runner's first act is to settle what a restart left: a turn it stops then was
        // never announced as at work. Only that stop wakes the asker, which may be an
        // agent no longer at work; a stop the user made must not start work again.
        let mut announced = false;
        // A member's task is the team's: its lease is moved on while this runner lives,
        // and what its turns do to the task is the team's to decide, not a report.
        let mut beat = team::is_member(&thread).then(team::Beat::new);
        loop {
            let event = match &mut beat {
                Some(beat) => beat.next_event(&self, &thread, &mut output).await,
                None => output.next_event().await,
            };
            let Some(event) = event else { break };
            announced |= matches!(
                event,
                ThreadEvent::TurnStarted(_)
                    | ThreadEvent::TurnRunning(_)
                    | ThreadEvent::TurnWaiting(_)
            );
            let report = match &event {
                _ if beat.is_some() => {
                    team::member_event(&self, &thread, &event, announced).await;
                    None
                }
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
                        if !announced {
                            Some(stopped)
                        } else {
                            let told = self.tell_stopped(&parent, &thread.id, &stopped).await;
                            if let Err(error) = told {
                                warn!("cannot report agent thread {}: {error:#}", thread.id);
                            }
                            None
                        }
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
            match self.mailboxes.agents_at_work(&self.state, &root).await {
                Ok(true) => {}
                Ok(false) => mailbox.emit(ThreadEvent::AgentsWorking(false)),
                Err(error) => warn!("cannot tell whether agents of {root} work: {error:#}"),
            }
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

    pub(crate) async fn answer(&self, thread_id: &str, message_id: Option<&str>) -> String {
        let Some(message_id) = message_id else {
            return "(no answer)".into();
        };
        match self.state.message_content(thread_id, message_id).await {
            Ok(Some(content)) => content,
            Ok(None) => "(no answer)".into(),
            Err(error) => {
                warn!("cannot read the answer of {thread_id}: {error:#}");
                "(no answer)".into()
            }
        }
    }
}
