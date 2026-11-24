//! `ThreadRunner`: the loop of one thread, taking its messages in and running its turns.

use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Agent, Inbox, Input, Mailbox, Mailboxes, Stop};
use crate::runtime::goal::action as goal;
use crate::runtime::{ThreadLock, model_choice::ModelChoice};
use crate::state::artifacts::ArtifactStore;
use crate::{
    config::live::LiveConfig,
    state::{State, Turn, TurnStatus},
    tools::{ToolRegistry, ToolScope},
};
use anyhow::Result;
use dusk_protocol::tools::Question;
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;

use super::resume::Restart;
use super::{ThreadInput, ThreadOutput};

/// Foreign runtimes cannot signal this inbox. Poll only while their jobs could
/// finish, without loading model context or doing other turn work on empty ticks.
const FOREIGN_JOB_POLL: Duration = Duration::from_secs(2);

/// Runs one thread; `Runtime` gives each active thread its own task with one of these.
pub struct ThreadRunner {
    pub agent: Agent,
    pub(super) inbox: Inbox,
    pub mailboxes: Arc<Mailboxes>,
    ends_when_idle: bool,
    /// The next automatic round waits for dependency completion, not a timer.
    pub goal_waiting: bool,
    /// Goal rounds in a row that called no tool.
    pub(crate) monologue: crate::runtime::goal::Monologue,
    /// The kickoff of the goal round queued, by its pending id: the next turn is the
    /// round's unless another message came with it or the goal stopped meanwhile.
    pub(crate) round_queued: Option<i64>,
    /// The last turn that started was a goal round's.
    pub(crate) round_turn: bool,
    /// Took itself out of the mailboxes, so another runner of the thread may be there now.
    left: bool,
    /// The question a restart asked while the runner waits for its answer.
    pub(super) deciding: Arc<Mutex<Option<Question>>>,
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
    pub health: Arc<crate::providers::Health>,
    pub skill_watch: Arc<crate::catalog::history::Watch>,
    pub skill_state: Arc<crate::state::State>,
}

impl ThreadRunner {
    pub fn new(setup: ThreadSetup) -> (Self, ThreadInput, ThreadOutput) {
        let (input_sender, inputs) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let deciding = Arc::default();
        let input = ThreadInput {
            state: Arc::clone(&setup.state),
            config: Arc::clone(&setup.config),
            model: Arc::clone(&setup.model),
            events: events.downgrade(),
            thread_id: setup.thread_id.clone(),
            inputs: input_sender,
            deciding: Arc::clone(&deciding),
        };
        let agent = Agent {
            state: setup.state,
            model: setup.model,
            config: setup.config,
            tools: Arc::new(setup.tools),
            scope: setup.scope,
            project: setup.project,
            pictures_refused: Mutex::new(None),
            effort_refused: Mutex::new(None),
            judge_effort_refused: Mutex::new(None),
            pace: Mutex::default(),
            repeats: Mutex::default(),
            asking: Mutex::new(Vec::new()),
            thread_id: setup.thread_id,
            compact_prompt: setup.compact_prompt,
            fork_prompt: setup.fork_prompt,
            modes_dir: setup.modes_dir,
            artifacts: setup.artifacts,
            depth: setup.depth,
            events,
            hooks: setup.hooks,
            health: setup.health,
            skill_watch: setup.skill_watch,
            skill_state: setup.skill_state,
        };
        let runner = Self {
            agent,
            inbox: Inbox::new(inputs),
            mailboxes: setup.mailboxes,
            ends_when_idle: setup.ends_when_idle,
            goal_waiting: false,
            monologue: Default::default(),
            round_queued: None,
            round_turn: false,
            left: false,
            deciding,
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
        let recovered = self.agent.recover().await?;
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
        // A chat whose newest turn failed shows the same row as when it ended. An agent's
        // old failure is not told again: its chat would take it for a new one.
        if self.agent.depth == 0
            && let Some(failure) = self.agent.state.last_failure(&self.agent.thread_id).await?
        {
            self.agent.emit(ThreadEvent::TurnFailed(failure));
        }
        // A chat opened while its agents work shows it is at work from the start.
        if self.agent.depth == 0
            && self
                .mailboxes
                .agents_at_work(&self.agent.state, &self.agent.thread_id)
                .await?
        {
            self.agent.emit(ThreadEvent::AgentsWorking(true));
        }
        // A turn a restart cut off goes on before anything sent meanwhile is taken in,
        // which would come between its step's calls and their results.
        match self.take_up(recovered).await? {
            Restart::GoOn(turn) => self.run_queued(Some(turn)).await?,
            Restart::Idle => {}
            Restart::Closed => return Ok(()),
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
            self.run_queued(None).await?;
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
    /// its dependencies have delivered their results. A `resumed` turn runs first.
    async fn run_queued(&mut self, mut resumed: Option<Turn>) -> Result<()> {
        let mut ran = false;
        let mut failed = false;
        // The last turn that ran, so a goal round is judged by how it ended.
        let mut ended: Option<String> = None;
        loop {
            let turn = match resumed.take() {
                // It goes on as it stood: a compaction would cut its step in two.
                Some(turn) => Ok(Some((turn, true))),
                None => match self.agent.compact_if_needed(None, &mut self.inbox).await {
                    Err(stop) => Err(stop),
                    Ok(_) => Ok(self.agent.start_turn().await?.map(|turn| (turn, false))),
                },
            };
            let stop = match turn {
                Err(stop) => Some(stop),
                Ok(Some((turn, resumes))) => {
                    ran = true;
                    ended = Some(turn.turn_id.clone());
                    // A turn the goal did not start, or that also reads what someone
                    // wrote, answers them; a goal paused or ended meanwhile has no round.
                    self.round_turn = self.round_queued.take().is_some()
                        && self.agent.active_goal().await?.is_some();
                    if !self.round_turn {
                        self.monologue.reset();
                    }
                    let (stop, status) = match resumes {
                        true => self.agent.resume_turn(&turn, &mut self.inbox).await?,
                        false => self.agent.run_turn(&turn, &mut self.inbox).await?,
                    };
                    failed = status == TurnStatus::Failed;
                    match stop {
                        Some(stop) => Some(stop),
                        None => self.agent.name_thread(&mut self.inbox).await,
                    }
                }
                // Only a turn that ran goes on to the next round: a context too
                // large to start one would start none forever.
                // A failing turn would fail again: the goal waits for the user.
                Ok(None) if ran && failed => {
                    self.pause_goal("the last turn failed").await?;
                    return Ok(());
                }
                Ok(None)
                    if (ran || self.goal_waiting || self.verification_pending().await?)
                        && self.next_goal_round(ended.take().as_deref()).await? =>
                {
                    ran = false;
                    continue;
                }
                Ok(None) => return Ok(()),
            };
            self.take_in().await?;
            if stop == Some(Stop::Cancelled) {
                return self.cancel_work().await;
            }
        }
    }

    /// A cancel also pauses an idle goal waiting for dependency completion.
    pub(super) async fn cancel_work(&mut self) -> Result<()> {
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
