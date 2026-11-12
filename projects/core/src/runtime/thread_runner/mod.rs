use super::{ThreadLock, model_choice::ModelChoice};
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Agent, Inbox, Input, Mailbox, Mailboxes, Stop};
use crate::runtime::goal::action as goal;
use crate::state::artifacts::ArtifactStore;
use crate::{
    config::live::LiveConfig,
    message::commands,
    state::{PendingMessage, State, TurnStatus},
    tools::{ToolRegistry, ToolScope},
};
use anyhow::Result;
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;

mod handle;
#[cfg(test)]
mod tests;

pub use handle::{ThreadInput, ThreadOutput};

/// Foreign runtimes cannot signal this inbox. Poll only while their jobs could
/// finish, without loading model context or doing other turn work on empty ticks.
const FOREIGN_JOB_POLL: Duration = Duration::from_secs(2);

/// Runs one thread; `Runtime` gives each active thread its own task with one of these.
pub struct ThreadRunner {
    pub agent: Agent,
    inbox: Inbox,
    pub mailboxes: Arc<Mailboxes>,
    ends_when_idle: bool,
    /// The next automatic round waits for dependency completion, not a timer.
    pub goal_waiting: bool,
    /// Took itself out of the mailboxes, so another runner of the thread may be there now.
    left: bool,
    /// Keeps other processes off the thread while this runner has it.
    _lock: ThreadLock,
}

/// Everything a running thread needs; the thread must exist.
pub struct ThreadSetup {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub thread_id: String,
    pub tools: ToolRegistry,
    pub scope: Arc<ToolScope>,
    /// The machine, the folder and the project's instructions.
    pub project: String,
    pub compact_prompt: PathBuf,
    pub fork_prompt: PathBuf,
    pub modes_dir: PathBuf,
    /// Where tool results keep their pictures, such as screenshots.
    pub artifacts: ArtifactStore,
    /// How many agents are above this thread; 0 for a chat.
    pub depth: usize,
    pub mailboxes: Arc<Mailboxes>,
    /// A subagent's runner ends when idle instead of waiting for its client.
    pub ends_when_idle: bool,
    pub lock: ThreadLock,
    pub hooks: Arc<crate::plugins::Hooks>,
}

impl ThreadRunner {
    pub fn new(setup: ThreadSetup) -> (Self, ThreadInput, ThreadOutput) {
        let (input_sender, inputs) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let input = ThreadInput {
            state: Arc::clone(&setup.state),
            config: Arc::clone(&setup.config),
            model: Arc::clone(&setup.model),
            events: events.downgrade(),
            thread_id: setup.thread_id.clone(),
            inputs: input_sender,
        };
        let agent = Agent {
            state: setup.state,
            model: setup.model,
            config: setup.config,
            tools: Arc::new(setup.tools),
            scope: setup.scope,
            project: setup.project,
            pictures_refused: Mutex::new(None),
            pace: Mutex::default(),
            repeats: Mutex::default(),
            asking: Mutex::new(None),
            thread_id: setup.thread_id,
            compact_prompt: setup.compact_prompt,
            fork_prompt: setup.fork_prompt,
            modes_dir: setup.modes_dir,
            artifacts: setup.artifacts,
            depth: setup.depth,
            events,
            hooks: setup.hooks,
        };
        let runner = Self {
            agent,
            inbox: Inbox::new(inputs),
            mailboxes: setup.mailboxes,
            ends_when_idle: setup.ends_when_idle,
            goal_waiting: false,
            left: false,
            _lock: setup.lock,
        };
        let output = ThreadOutput {
            events: event_receiver,
        };
        (runner, input, output)
    }

    /// How other threads reach this one while it runs.
    pub fn mailbox(&self, input: &ThreadInput) -> Mailbox {
        Mailbox {
            input: input.inputs.downgrade(),
            events: self.agent.events.downgrade(),
            agent: self.ends_when_idle,
            context: self.agent.context_source(),
        }
    }

    /// Runs until every `ThreadInput` is dropped and the queue is done, or for a
    /// subagent until it is idle. An error ends the thread and is reported to the client
    /// as `Failed`. Returns whether it ended without one.
    pub async fn run(mut self) -> bool {
        let served = self.serve().await;
        if let Err(error) = &served {
            self.agent.emit(ThreadEvent::Failed(format!("{error:#}")));
        }
        if self.ends_when_idle && !self.left {
            self.mailboxes.remove(&self.agent.thread_id).await;
        }
        served.is_ok()
    }

    async fn serve(&mut self) -> Result<()> {
        self.agent.recover().await?;
        // The client sees the context size and the goal before the first turn, too.
        self.agent.report_tokens().await;
        if self.agent.depth == 0
            && let Some(goal) = self.agent.state.goal(&self.agent.thread_id).await?
        {
            let note = goal::describe(&goal);
            self.agent.emit(ThreadEvent::Goal {
                goal: Some(goal),
                note,
                started: None,
            });
        }
        loop {
            if self.ends_when_idle && self.mailboxes.is_closed() {
                return Ok(());
            }
            if self.agent.model.current().is_none() {
                self.agent.emit(ThreadEvent::Idle);
                if self.ends_when_idle {
                    return Ok(());
                }
                match self.inbox.wait().await {
                    None => return Ok(()),
                    Some(Input::Cancel) => self.cancel_work().await?,
                    Some(_) => {}
                }
                continue;
            }
            self.take_in().await?;
            self.run_queued().await?;
            if self.has_pending().await? {
                continue;
            }
            let (pending, foreign_jobs) = self
                .agent
                .tools
                .job_wake_state(&self.agent.thread_id)
                .await?;
            if pending {
                continue;
            }
            self.agent.emit(ThreadEvent::Idle);
            if self.ends_when_idle
                && !foreign_jobs
                && !self
                    .agent
                    .tools
                    .has_running_jobs(&self.agent.thread_id)
                    .await
            {
                if self.leave_if_idle().await? {
                    return Ok(());
                }
                continue;
            }
            // Locally owned processes use the existing event-driven inbox. A
            // reopened thread also watches durable results from other runtimes.
            match self.wait_for_work(foreign_jobs).await? {
                None => return Ok(()),
                Some(Input::Cancel) => self.cancel_work().await?,
                Some(_) => {}
            }
        }
    }

    async fn wait_for_work(&mut self, mut foreign_jobs: bool) -> Result<Option<Input>> {
        loop {
            if !foreign_jobs {
                let input = self.inbox.wait().await;
                // Real deliveries send Sent first. A lifecycle-only wake must not
                // admit agent messages whose normal wake was withheld by its limit.
                if matches!(input, Some(Input::DependencyFinished))
                    && (!self.goal_waiting || self.has_pending().await?)
                {
                    continue;
                }
                return Ok(input);
            }
            tokio::select! {
                input = self.inbox.wait() => {
                    if matches!(input, Some(Input::DependencyFinished))
                        && (!self.goal_waiting || self.has_pending().await?)
                    {
                        continue;
                    }
                    return Ok(input);
                },
                _ = tokio::time::sleep(FOREIGN_JOB_POLL) => {}
            }
            let (pending, still_foreign) = self
                .agent
                .tools
                .job_wake_state(&self.agent.thread_id)
                .await?;
            if pending {
                return Ok(Some(Input::Sent));
            }
            // If the last foreign job completed before this SELECT, its pending
            // event is in the same snapshot. Otherwise a later tick observes it.
            foreign_jobs = still_foreign;
        }
    }

    /// Runs turns until nothing is queued, the user cancels or the context does not fit.
    /// The context is compacted first when it has grown too large. What was sent
    /// meanwhile is taken in after each turn, so the next one answers it.
    /// An active goal starts the next turn itself once nothing else is queued and
    /// its dependencies have delivered their results.
    async fn run_queued(&mut self) -> Result<()> {
        let mut ran = false;
        let mut failed = false;
        loop {
            let stop = match self.agent.compact_if_needed(None, &mut self.inbox).await {
                Some(stop) => Some(stop),
                None => match self.agent.start_turn().await? {
                    Some(turn) => {
                        ran = true;
                        let (stop, status) = self.agent.run_turn(&turn, &mut self.inbox).await?;
                        failed = status == TurnStatus::Failed;
                        match stop {
                            Some(stop) => Some(stop),
                            None => self.agent.name_thread(&mut self.inbox).await,
                        }
                    }
                    // Only a turn that ran goes on to the next round: a context too
                    // large to start one would start none forever.
                    // A failing turn would fail again: the goal waits for the user.
                    None if ran && failed => {
                        self.pause_goal("the last turn failed").await?;
                        return Ok(());
                    }
                    None if (ran || self.goal_waiting || self.verification_pending().await?)
                        && self.next_goal_round().await? =>
                    {
                        ran = false;
                        continue;
                    }
                    None => return Ok(()),
                },
            };
            self.take_in().await?;
            if stop == Some(Stop::Cancelled) {
                return self.cancel_work().await;
            }
        }
    }

    /// Takes in what clients sent, oldest first: messages join the thread, commands run.
    /// A command can take a while, so whatever arrived meanwhile is taken in as well.
    pub async fn take_in(&mut self) -> Result<()> {
        loop {
            let pending = self
                .agent
                .state
                .pending_messages(&self.agent.thread_id)
                .await?;
            if pending.is_empty() {
                return Ok(());
            }
            for message in pending {
                self.take(message).await?;
            }
        }
    }

    async fn take(&mut self, pending: PendingMessage) -> Result<()> {
        let state = Arc::clone(&self.agent.state);
        // Another agent's message is never a command, whatever it starts with.
        let command = if pending.from_thread_id.is_some() || pending.from_job_id.is_some() {
            None
        } else {
            commands::parse(&pending.content)
        };
        let Some(command) = command else {
            let message = state.admit_pending(&self.agent.thread_id, &pending).await?;
            if message.from_thread_id.is_some() {
                sqlx::query("UPDATE goals SET work_epoch = work_epoch + 1 WHERE thread_id = ? AND approved_at IS NOT NULL")
                    .bind(&self.agent.thread_id).execute(&state.pool).await?;
            } else if message.from_job_id.is_none() {
                sqlx::query("UPDATE goals SET awaiting_user = 0 WHERE thread_id = ?")
                    .bind(&self.agent.thread_id)
                    .execute(&state.pool)
                    .await?;
            }
            if message.from_job_id.is_none() {
                self.agent.title_from_first_message(&message).await?;
            }
            if message.from_thread_id.is_none() && message.from_job_id.is_none() {
                let details = serde_json::json!({ "text": message.content });
                self.agent
                    .hook_notes(crate::plugins::HookEvent::UserMessage, details)
                    .await?;
            }
            self.agent.emit(ThreadEvent::Queued {
                message_id: message.message_id.clone(),
            });
            if message.from_job_id.is_some() {
                self.agent.emit(ThreadEvent::FromJob(message));
            } else if message.from_thread_id.is_some() {
                self.agent.emit(ThreadEvent::FromAgent(message));
            } else {
                self.agent.emit(ThreadEvent::UserMessage(message));
            }
            return Ok(());
        };
        match command {
            Err(error) => self.agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            }),
            Ok(command) => commands::run_in_thread(command, &self.agent, &mut self.inbox).await,
        }
        // Removed only once done: a command cut short by a crash runs again on restart.
        state.remove_pending(pending.pending_id).await
    }

    /// A cancel also pauses an idle goal waiting for dependency completion.
    async fn cancel_work(&mut self) -> Result<()> {
        self.goal_waiting = false;
        self.pause_goal("stopped by the user").await?;
        self.cancel_agents().await
    }

    /// Stopping a thread stops every agent below it, including those under an agent that
    /// is idle meanwhile, so none of them reports back and wakes the thread again.
    /// Stops every agent below: those at work, and those waiting for a free slot, whose
    /// waiting messages are dropped.
    async fn cancel_agents(&self) -> Result<()> {
        let below = self.agent.state.descendant_threads(&self.agent.thread_id);
        for thread_id in below.await? {
            match self.mailboxes.get(&thread_id).await {
                Some(mailbox) => {
                    mailbox.send(Input::Cancel);
                }
                None => self.agent.state.drop_pending(&thread_id).await?,
            }
        }
        Ok(())
    }

    /// Ends a subagent's runner unless a message came in meanwhile. Checked under the
    /// mailboxes lock, which a sender holds after storing its message, so a message
    /// either finds this runner live or finds it gone and starts a new one.
    async fn leave_if_idle(&mut self) -> Result<bool> {
        let mut live = self.mailboxes.lock().await;
        if self.has_pending().await? {
            return Ok(false);
        }
        live.remove(&self.agent.thread_id);
        self.left = true;
        Ok(true)
    }

    async fn has_pending(&self) -> Result<bool> {
        self.agent.state.has_pending(&self.agent.thread_id).await
    }
}
