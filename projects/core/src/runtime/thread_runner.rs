//! A live thread. Messages can be sent at any time; the agent runs one turn at a time
//! over everything queued. `send_now` stops the running turn, keeping what it wrote,
//! so the next turn starts at once with the new message.
//!
//! A send is confirmed once the message is stored as pending. The thread takes pending
//! messages in when it is free: right away when idle, after the running turn otherwise,
//! so the thread always reads in the order things happened. A message that is a command,
//! such as `/compact`, runs at that point instead of joining the thread. Messages from
//! other agents also join a running turn, between its steps.
//!
//! A subagent's runner ends once it is idle; a new message starts it again. Stopping a
//! thread stops the agents it asked.

use std::{path::PathBuf, sync::Arc, time::Duration};

use parking_lot::Mutex;

use anyhow::{Result, ensure};
use tokio::sync::mpsc;

use super::{ThreadLock, model_choice::ModelChoice};
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Agent, Inbox, Input, Mailbox, Mailboxes, Stop};
use crate::runtime::goal::action as goal;
use crate::state::artifacts::ArtifactStore;
use crate::{
    config::live::LiveConfig,
    message::commands,
    state::{PendingMessage, State, TurnStatus, estimate_tokens},
    tools::{ToolRegistry, ToolScope},
};

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

/// The client's side for sending. Dropping every clone ends the thread once the queue is done.
#[derive(Clone)]
pub struct ThreadInput {
    state: Arc<State>,
    config: Arc<LiveConfig>,
    model: Arc<ModelChoice>,
    /// Weak, so the client's output ends when the runner does and the client can tell.
    events: mpsc::WeakUnboundedSender<ThreadEvent>,
    thread_id: String,
    inputs: mpsc::UnboundedSender<Input>,
}

/// The client's side for receiving.
pub struct ThreadOutput {
    events: mpsc::UnboundedReceiver<ThreadEvent>,
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
            tools: setup.tools,
            scope: setup.scope,
            project: setup.project,
            pictures_refused: Mutex::new(None),
            pace: Mutex::default(),
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

impl ThreadInput {
    /// Queues a message; it is answered after the running turn, together with other
    /// queued ones. Once this returns, the message is stored.
    pub async fn send(&self, text: impl Into<String>) -> Result<()> {
        self.deliver(text.into(), Input::Sent).await
    }

    /// Queues a message and stops the running turn, so the agent starts on it at once.
    pub async fn send_now(&self, text: impl Into<String>) -> Result<()> {
        self.deliver(text.into(), Input::SentNow).await
    }

    /// Stops the running turn; queued messages wait for the next one.
    pub async fn cancel(&self) -> Result<()> {
        // Serializes cancellation with the verification commit. Once this returns,
        // an active goal cannot complete on an earlier request.
        sqlx::query("UPDATE goals SET status = 'paused', note = 'stopped by the user',
            verification = CASE WHEN verification IN ('pending', 'checking') THEN 'gaps' ELSE verification END,
            verification_note = CASE WHEN verification IN ('pending', 'checking') THEN 'Verification cancelled; no pass was recorded.' ELSE verification_note END
            WHERE thread_id = ? AND status = 'active'")
            .bind(&self.thread_id).execute(&self.state.pool).await?;
        ensure!(self.inputs.send(Input::Cancel).is_ok(), "thread has ended");
        Ok(())
    }

    async fn deliver(&self, text: String, input: Input) -> Result<()> {
        ensure!(!self.inputs.is_closed(), "thread has ended");
        // A message that alone is past the input limit could never be answered.
        let config = self.config.current();
        let limit = self
            .model
            .current()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(&config)
            });
        let tokens = estimate_tokens(&text);
        ensure!(
            tokens <= limit,
            "the message is about {tokens} tokens, past the input limit of {limit}"
        );
        if self.model.current().is_none() {
            let message = self.state.add_local_unsent(&self.thread_id, &text).await?;
            if let Some(events) = self.events.upgrade() {
                let _ = events.send(ThreadEvent::LocalUnsent(message));
                let _ = events.send(ThreadEvent::ProviderUnavailable);
            }
            return Ok(());
        }
        self.state.add_pending(&self.thread_id, &text).await?;
        // Stored either way: a thread that has just ended takes it in on its next start.
        let _ = self.inputs.send(input);
        Ok(())
    }
}

impl ThreadOutput {
    /// `None` once the thread has ended.
    pub async fn next_event(&mut self) -> Option<ThreadEvent> {
        self.events.recv().await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use serde_json::{Value, json};

    use super::*;
    use crate::providers::{ModelEvent, ToolCall};
    use crate::state::{Goal, GoalStatus, Turn};
    use crate::{
        BoxFuture,
        providers::{
            ChatMessage, ChatRequest, ChatStream, FinishReason, Model, ModelProvider, Transient,
        },
        runtime::model_choice::Choice,
        state::{MessageRole, TurnStatus},
        testing::{temp_dir, temp_live_config, temp_live_config_with, temp_state},
        tools::{Tool, ToolContext},
    };

    /// Each model call takes the next script; `Hang` never finishes, like a slow model.
    /// The messages of every request are kept.
    struct FakeProvider {
        scripts: Mutex<VecDeque<Vec<Step>>>,
        requests: Mutex<Vec<Vec<ChatMessage>>>,
        efforts: Mutex<Vec<Option<crate::config::Effort>>>,
    }

    #[derive(Clone)]
    enum Step {
        Text(&'static str),
        /// Calls the `echo` tool with this text.
        Call(&'static str),
        Tool(String, Value),
        Hang,
        Done,
        /// The call fails before any output, in a way worth retrying.
        Fail,
        /// The connection drops where this step stands.
        Break,
    }

    struct FakeStream {
        steps: VecDeque<Step>,
        pending: VecDeque<ModelEvent>,
    }

    /// Returns its `text` argument.
    struct Echo;

    impl ModelProvider for FakeProvider {
        fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn stream_chat<'a>(
            &'a self,
            request: ChatRequest<'a>,
        ) -> BoxFuture<'a, Box<dyn ChatStream>> {
            self.requests
                .lock()
                .unwrap()
                .push(request.messages.to_vec());
            // Agent steps only: titles and summaries go without tools.
            if !request.tools.is_empty() {
                self.efforts.lock().unwrap().push(request.effort);
            }
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            if let Some(Step::Fail) = script.first() {
                return Box::pin(async { Err(Transient("server busy".into()).into()) });
            }
            let stream = FakeStream {
                steps: script.into(),
                pending: VecDeque::new(),
            };
            Box::pin(async move { Ok(Box::new(stream) as Box<dyn ChatStream>) })
        }
    }

    impl ChatStream for FakeStream {
        fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
            Box::pin(async move {
                if let Some(event) = self.pending.pop_front() {
                    return Ok(Some(event));
                }
                Ok(match self.steps.pop_front() {
                    Some(Step::Text(text)) => Some(ModelEvent::Text(text.into())),
                    Some(Step::Call(text)) => {
                        self.pending.push_back(ModelEvent::ToolArguments {
                            index: 0,
                            fragment: json!({ "text": text }).to_string(),
                        });
                        Some(ModelEvent::ToolCall {
                            index: 0,
                            id: "call_1".into(),
                            name: "echo".into(),
                        })
                    }
                    Some(Step::Tool(name, arguments)) => {
                        self.pending.push_back(ModelEvent::ToolArguments {
                            index: 0,
                            fragment: arguments.to_string(),
                        });
                        Some(ModelEvent::ToolCall {
                            index: 0,
                            id: "goal_call".into(),
                            name,
                        })
                    }
                    Some(Step::Hang) => std::future::pending().await,
                    Some(Step::Done) => Some(ModelEvent::Finished(FinishReason::Stop)),
                    Some(Step::Break) => return Err(Transient("connection reset".into()).into()),
                    Some(Step::Fail) | None => None,
                })
            })
        }
    }

    impl Tool for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn description(&self, _context: &ToolContext) -> String {
            "Returns its text.".into()
        }

        fn parameters(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
            Box::pin(async move { Ok(arguments["text"].as_str().unwrap_or_default().to_owned()) })
        }
    }

    fn provider(scripts: Vec<Vec<Step>>) -> Arc<FakeProvider> {
        Arc::new(FakeProvider {
            scripts: Mutex::new(scripts.into()),
            requests: Mutex::new(Vec::new()),
            efforts: Mutex::new(Vec::new()),
        })
    }

    fn model(context_window: Option<u64>) -> Model {
        Model {
            id: "fake".into(),
            context_window,
            max_output: None,
        }
    }

    const MAIN_PROMPT: &str = "You are the main agent of duskcode, working with the user in a chat.\n\
         Answer directly and concisely, in the language the user writes in.\n";

    /// A chat as `start_thread` makes it: its first message is the mode's prompt.
    async fn thread_state(name: &str) -> Arc<State> {
        let state = Arc::new(temp_state(name).await);
        state.create_thread("thread", "test").await.unwrap();
        state
            .add_system_message("thread", MAIN_PROMPT)
            .await
            .unwrap();
        state
    }

    fn live_config(name: &str) -> Arc<LiveConfig> {
        Arc::new(temp_live_config(name))
    }

    fn runner(
        state: &Arc<State>,
        provider: &Arc<FakeProvider>,
        config: &Arc<LiveConfig>,
        model: Model,
    ) -> (ThreadRunner, ThreadInput, ThreadOutput) {
        runner_with(state, provider, config, model, Box::new(Echo))
    }

    /// A runner whose only tool is `tool`, which the scripts call as `echo`.
    fn runner_with(
        state: &Arc<State>,
        provider: &Arc<FakeProvider>,
        config: &Arc<LiveConfig>,
        model: Model,
        tool: Box<dyn Tool>,
    ) -> (ThreadRunner, ThreadInput, ThreadOutput) {
        let root = temp_dir("runner_scope");
        std::fs::create_dir_all(&root).unwrap();
        let mut tools = ToolRegistry::default();
        tools.add(tool);
        tools.set_depth(0);
        let choice = Choice {
            provider_name: "fake".into(),
            provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
            model,
        };
        // The size of the real prompt must not decide what fits in these small limits.
        std::fs::create_dir_all(root.join("modes")).unwrap();
        std::fs::write(root.join("modes/MAIN.md"), MAIN_PROMPT).unwrap();
        ThreadRunner::new(ThreadSetup {
            state: Arc::clone(state),
            model: Arc::new(ModelChoice::new(choice)),
            config: Arc::clone(config),
            thread_id: "thread".into(),
            tools,
            scope: Arc::new(ToolScope::new(&root, &root).unwrap()),
            project: String::new(),
            compact_prompt: root.join("COMPACT.md"),
            fork_prompt: root.join("FORK.md"),
            modes_dir: root.join("modes"),
            artifacts: ArtifactStore::new(root.join("artifacts")),
            depth: 0,
            mailboxes: Arc::default(),
            ends_when_idle: false,
            lock: ThreadLock::take(&root).unwrap(),
            hooks: Arc::default(),
        })
    }

    async fn next_turn_finished(output: &mut ThreadOutput) -> Turn {
        loop {
            if let Some(ThreadEvent::TurnFinished(turn)) = output.next_event().await {
                return turn;
            }
        }
    }

    /// The chat after its opening mode prompt.
    async fn transcript(state: &State) -> Vec<(MessageRole, String)> {
        let messages = state.list_messages("thread").await.unwrap();
        messages
            .into_iter()
            .skip(1)
            .map(|message| (message.role, message.content))
            .collect()
    }

    #[tokio::test]
    async fn a_goal_goes_on_round_after_round_until_they_run_out() {
        let state = thread_state("goal_rounds").await;
        let config = Arc::new(temp_live_config_with(
            "goal_config",
            "limits:\n  max_goal_rounds: 2\n",
        ));
        let draft = state.set_goal("thread", "build it").await.unwrap();
        let draft = state
            .propose_goal(
                "thread",
                &draft.revision,
                vec![crate::state::GoalCriterion {
                    id: "result".into(),
                    description: "review result".into(),
                    check: crate::state::GoalCheck::UserConfirmation,
                }],
            )
            .await
            .unwrap();
        let approve = format!("/goal approve {}", draft.revision);
        let answer = vec![Step::Text("working"), Step::Done];
        // One more for the title the thread gets after its first turn.
        let provider = provider(vec![answer.clone(); 4]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            input.send(approve).await.unwrap();
            loop {
                if let Some(ThreadEvent::Goal {
                    goal: Some(goal), ..
                }) = output.next_event().await
                    && goal.status == GoalStatus::Paused
                    && goal.approved
                {
                    return goal;
                }
            }
        };

        let (_, goal) = tokio::join!(runner.run(), client);
        assert_eq!(goal.rounds, 2);
        assert_eq!(goal.note.as_deref(), Some("all 2 rounds used"));
        let users: Vec<_> = transcript(&state)
            .await
            .into_iter()
            .filter(|(role, _)| *role == MessageRole::User)
            .map(|(_, text)| text)
            .collect();
        assert_eq!(
            users,
            [
                "Goal: build it",
                "Continue toward the goal (round 1 of 2): build it",
                "Continue toward the goal (round 2 of 2): build it",
            ]
        );
        let requests = provider.requests.lock().unwrap();
        assert!(matches!(
            requests[0].last(),
            Some(ChatMessage::System(text)) if text.starts_with("Standing goal")
        ));
    }

    async fn verified_fixture(state: &State) -> (Goal, String) {
        let goal = state.set_goal("thread", "build it").await.unwrap();
        let goal = state
            .propose_goal(
                "thread",
                &goal.revision,
                vec![crate::state::GoalCriterion {
                    id: "exit".into(),
                    description: "approved check".into(),
                    check: crate::state::GoalCheck::Command {
                        command: "exit 0".into(),
                    },
                }],
            )
            .await
            .unwrap();
        let goal = state.approve_goal("thread", &goal.revision).await.unwrap();
        state
            .add_user_message("thread", "proof", "run the approved check")
            .await
            .unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        state
            .record_tool_calls(
                &turn.turn_id,
                crate::state::TurnAnswer {
                    content: String::new(),
                    reasoning: None,
                },
                vec![ToolCall {
                    id: "check".into(),
                    name: "run_command".into(),
                    arguments: json!({"command":"exit 0"}),
                }],
            )
            .await
            .unwrap();
        let root = temp_dir("goal-real-proof");
        std::fs::create_dir_all(&root).unwrap();
        let scope = ToolScope::new(&root, &root).unwrap();
        let tool = crate::tools::RunCommand::default();
        let result = tool
            .call_result(
                json!({"command":"exit 0"}),
                crate::testing::tool_context(&scope),
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        let result = state
            .record_tool_result_with_outcome(
                &turn.turn_id,
                "check",
                result.content,
                result.artifacts,
                "succeeded",
                result.exit_code,
            )
            .await
            .unwrap();
        state
            .finish_turn(
                &turn.turn_id,
                crate::state::TurnOutcome::Completed(crate::state::TurnAnswer {
                    content: "check executed".into(),
                    reasoning: None,
                }),
            )
            .await
            .unwrap();
        (goal, format!("tool:{}", result.message_id))
    }

    #[tokio::test]
    async fn runner_routes_a_completion_request_through_saved_evidence_review() {
        let state = thread_state("goal-runner-review").await;
        let (goal, result_id) = verified_fixture(&state).await;
        let config = live_config("goal-runner-review-config");
        let provider = provider(vec![
            vec![
                Step::Tool(
                    "goal".into(),
                    json!({"action":"request_verification","revision":goal.revision,
                "evidence":[{"criterion_id":"exit","result_id":result_id}]}),
                ),
                Step::Done,
            ],
            vec![Step::Text("I think it is done"), Step::Done],
            vec![Step::Text("thread name"), Step::Done],
        ]);
        let (runner, input, mut output) = runner_with(
            &state,
            &provider,
            &config,
            model(None),
            Box::new(crate::tools::GoalTool::new(
                Arc::clone(&state),
                "thread".into(),
            )),
        );
        let client = async move {
            input.send("finish the goal").await.unwrap();
            while let Some(event) = output.next_event().await {
                if let ThreadEvent::Goal {
                    goal: Some(goal), ..
                } = event
                    && goal.status == GoalStatus::Done
                {
                    assert_eq!(goal.verification, "passed");
                    return;
                }
            }
            panic!("runtime did not publish verified completion");
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(runner.run(), client);
        })
        .await
        .unwrap();
        assert_eq!(
            state.goal("thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }

    #[tokio::test]
    async fn cancelling_after_the_model_requests_verification_prevents_completion() {
        let state = thread_state("goal-runner-cancel").await;
        let (goal, result_id) = verified_fixture(&state).await;
        let config = live_config("goal-runner-cancel-config");
        let provider = provider(vec![
            vec![
                Step::Tool(
                    "goal".into(),
                    json!({"action":"request_verification","revision":goal.revision,
                "evidence":[{"criterion_id":"exit","result_id":result_id}]}),
                ),
                Step::Done,
            ],
            vec![Step::Hang],
        ]);
        let (runner, input, mut output) = runner_with(
            &state,
            &provider,
            &config,
            model(None),
            Box::new(crate::tools::GoalTool::new(
                Arc::clone(&state),
                "thread".into(),
            )),
        );
        let client = async move {
            input.send("finish the goal").await.unwrap();
            while let Some(event) = output.next_event().await {
                if matches!(event, ThreadEvent::ToolFinished { .. }) {
                    input.cancel().await.unwrap();
                }
                if let ThreadEvent::TurnFinished(turn) = event {
                    assert_eq!(turn.status, TurnStatus::Interrupted);
                    return;
                }
            }
            panic!("cancel did not finish the turn");
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(runner.run(), client);
        })
        .await
        .unwrap();
        let goal = state.goal("thread").await.unwrap().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert_ne!(goal.verification, "passed");
    }

    #[tokio::test]
    async fn a_draft_blocks_non_goal_tools_even_when_the_model_calls_them() {
        let state = thread_state("goal-draft-block").await;
        state.set_goal("thread", "do work").await.unwrap();
        let config = live_config("goal-draft-block-config");
        let provider = provider(vec![
            vec![Step::Call("do work"), Step::Done],
            vec![Step::Text("criteria needed"), Step::Done],
            vec![Step::Text("title"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            input.send("propose criteria").await.unwrap();
            next_turn_finished(&mut output).await;
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(runner.run(), client);
        })
        .await
        .unwrap();
        let outcome: String =
            sqlx::query_scalar("SELECT outcome FROM tool_executions WHERE call_id = 'call_1'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(outcome, "failed");
        assert!(
            state
                .list_messages("thread")
                .await
                .unwrap()
                .iter()
                .any(|m| m.role == MessageRole::Tool && m.content.contains("execution is blocked"))
        );
    }

    #[tokio::test]
    async fn send_now_interrupts_and_answers_the_new_message_at_once() {
        let state = thread_state("send_now").await;
        let config = live_config("runner_config");
        let provider = provider(vec![
            vec![Step::Text("partial"), Step::Hang],
            vec![Step::Text("second"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            input.send("first").await.unwrap();
            while output.next_event().await
                != Some(ThreadEvent::Model(ModelEvent::Text("partial".into())))
            {}
            input.send_now("now").await.unwrap();
            let first = next_turn_finished(&mut output).await;
            let second = next_turn_finished(&mut output).await;
            (first.status, second.status)
        };

        let (_, statuses) = tokio::join!(runner.run(), client);
        assert_eq!(statuses, (TurnStatus::Interrupted, TurnStatus::Completed));
        assert_eq!(
            transcript(&state).await,
            [
                (MessageRole::User, "first".into()),
                (MessageRole::Assistant, "partial".into()),
                (MessageRole::User, "now".into()),
                (MessageRole::Assistant, "second".into()),
            ]
        );
    }

    #[tokio::test]
    async fn cancel_stops_the_turn_and_waits() {
        let state = thread_state("cancel").await;
        let config = live_config("runner_config");
        let provider = provider(vec![vec![Step::Hang]]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            input.send("first").await.unwrap();
            while !matches!(output.next_event().await, Some(ThreadEvent::TurnStarted(_))) {}
            input.cancel().await.unwrap();
            let turn = next_turn_finished(&mut output).await;
            let tokens = output.next_event().await;
            assert!(
                matches!(tokens, Some(ThreadEvent::Tokens { .. })),
                "{tokens:?}"
            );
            assert_eq!(output.next_event().await, Some(ThreadEvent::Idle));
            turn.status
        };

        let (_, status) = tokio::join!(runner.run(), client);
        assert_eq!(status, TurnStatus::Interrupted);
        assert_eq!(transcript(&state).await.len(), 1, "nothing was written");
    }

    #[tokio::test]
    async fn a_message_sent_before_a_crash_is_answered_on_the_next_start() {
        let state = thread_state("pending_crash").await;
        // Confirmed to the client, but the process stopped before the thread took it in.
        state.add_pending("thread", "still there?").await.unwrap();
        let config = live_config("runner_config");
        let provider = provider(vec![vec![Step::Text("yes"), Step::Done]]);
        let (runner, input, _output) = runner(&state, &provider, &config, model(None));
        drop(input);

        runner.run().await;

        let transcript = transcript(&state).await;
        assert_eq!(
            transcript,
            [
                (MessageRole::User, "still there?".to_owned()),
                (MessageRole::Assistant, "yes".to_owned())
            ]
        );
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_model_past_the_output_limit_is_stopped_and_its_text_kept() {
        let state = thread_state("output_limit").await;
        let config = Arc::new(temp_live_config_with("output_limit_config", ""));
        // The third piece takes the output to 3 tokens; the stream must not be read on.
        let script = vec![
            Step::Text("abcd"),
            Step::Text("efgh"),
            Step::Text("ijkl"),
            Step::Hang,
        ];
        let provider = provider(vec![script]);
        let capped = Model {
            max_output: Some(2),
            ..model(None)
        };
        let (runner, input, mut output) = runner(&state, &provider, &config, capped);
        input.send("write a lot").await.unwrap();
        drop(input);

        runner.run().await;

        let mut reached = false;
        let mut status = None;
        while let Some(event) = output.next_event().await {
            match event {
                ThreadEvent::OutputLimitReached => reached = true,
                ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
                _ => {}
            }
        }
        assert!(reached);
        assert_eq!(status, Some(TurnStatus::Interrupted));
        let transcript = transcript(&state).await;
        assert_eq!(
            transcript[1],
            (MessageRole::Assistant, "abcdefghijkl".into())
        );
    }

    #[tokio::test]
    async fn a_growing_context_is_compacted_within_the_turn() {
        let state = thread_state("compact_auto").await;
        let config = live_config("runner_config");
        let long = "a tool result long enough to push the context past the limit, \
                    and longer still, since the mode's prompt is in the context too, \
                    so it takes a few more words than before to get there";
        let provider = provider(vec![
            vec![Step::Call(long), Step::Done],
            vec![Step::Text("used the tool"), Step::Done],
            vec![Step::Text("done"), Step::Done],
        ]);
        // About 50 tokens before the step, the prompt included, and about 130 after it:
        // past 80% of 120, within 120 only after the summary.
        let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(120)));
        input.send("go").await.unwrap();
        drop(input);

        runner.run().await;

        let mut compaction = None;
        let mut status = None;
        while let Some(event) = output.next_event().await {
            match event {
                ThreadEvent::CompactionFinished(finished) => compaction = Some(finished),
                ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
                _ => {}
            }
        }
        assert!(compaction.is_some(), "compacted on its own");
        assert_eq!(status, Some(TurnStatus::Completed));
        let requests = provider.requests.lock().unwrap();
        assert_eq!(
            requests[2].len(),
            2,
            "the mode's prompt and the summary are left"
        );
        assert!(matches!(requests[2][0], ChatMessage::System(_)));
    }

    /// Like a subagent writing back while the asking turn runs a tool.
    struct WriteBack(Arc<State>);

    impl Tool for WriteBack {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn description(&self, _context: &ToolContext) -> String {
            "Lets the subagent write back.".into()
        }

        fn parameters(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call<'a>(&'a self, _arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
            Box::pin(async move {
                let sent = self.0.add_pending_from("thread", "found it", Some("child"));
                sent.await?;
                Ok("asked".into())
            })
        }
    }

    #[tokio::test]
    async fn an_agent_message_joins_the_running_turn_and_starts_no_other() {
        let state = thread_state("agent_message").await;
        let child = state.create_agent_thread("child", "thread", "general", "Ember-0c");
        child.await.unwrap();
        let config = live_config("runner_config");
        let provider = provider(vec![
            vec![Step::Call("go"), Step::Done],
            vec![Step::Text("it found it"), Step::Done],
        ]);
        let tool = Box::new(WriteBack(Arc::clone(&state)));
        let (runner, input, _output) = runner_with(&state, &provider, &config, model(None), tool);
        input.send("ask the child").await.unwrap();
        drop(input);

        runner.run().await;

        let second_step = provider.requests.lock().unwrap()[1].clone();
        let header = "[message from Ember-0c, thread child]\nfound it".to_owned();
        assert_eq!(second_step.last(), Some(&ChatMessage::User(header)));
        let messages = state.list_messages("thread").await.unwrap();
        let from = messages
            .iter()
            .find(|message| message.content == "found it");
        assert_eq!(from.unwrap().from_thread_id.as_deref(), Some("child"));
        assert_eq!(
            state.count_turns("thread").await.unwrap(),
            1,
            "no second turn"
        );
    }

    #[tokio::test]
    async fn an_answer_cut_off_keeps_what_was_written() {
        let state = thread_state("answer_cut").await;
        let config = live_config("runner_config");
        let provider = provider(vec![vec![Step::Text("half an answer"), Step::Break]]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("tell me").await.unwrap();
        drop(input);

        runner.run().await;

        let mut cut = false;
        while let Some(event) = output.next_event().await {
            cut |= matches!(event, ThreadEvent::AnswerCut { .. });
        }
        assert!(cut, "the client hears why the answer stopped");
        let turn = state.list_messages("thread").await.unwrap();
        assert_eq!(
            turn.last().map(|message| message.content.as_str()),
            Some("half an answer")
        );
        assert_eq!(state.count_turns("thread").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn runs_tools_until_the_model_answers() {
        let state = thread_state("tool_loop").await;
        let config = live_config("runner_config");
        let provider = provider(vec![
            vec![Step::Call("ping"), Step::Done],
            vec![Step::Text("got ping"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("use the tool").await.unwrap();
        drop(input);

        runner.run().await;

        let mut finished = None;
        while let Some(event) = output.next_event().await {
            if let ThreadEvent::ToolFinished { result, .. } = event {
                finished = Some(result.content);
            }
        }
        assert_eq!(finished.as_deref(), Some("ping"));
        assert_eq!(
            transcript(&state).await,
            [
                (MessageRole::User, "use the tool".into()),
                (MessageRole::Assistant, String::new()),
                (MessageRole::Tool, "ping".into()),
                (MessageRole::Assistant, "got ping".into()),
            ]
        );
    }

    /// The first step plans and a step after a failure works it out at the effort set;
    /// steps that carry out the plan think lightly.
    #[tokio::test]
    async fn a_turn_thinks_once_then_acts() {
        use crate::config::Effort;
        let state = thread_state("pace").await;
        let config = Arc::new(temp_live_config_with("pace_config", "effort: high\n"));
        // A call that works, one to a tool that is not there, then the answer.
        let provider = provider(vec![
            vec![Step::Call("one"), Step::Done],
            vec![Step::Tool("missing".into(), json!({})), Step::Done],
            vec![Step::Text("done"), Step::Done],
        ]);
        let (runner, input, _output) = runner(&state, &provider, &config, model(None));
        input.send("go").await.unwrap();
        drop(input);
        runner.run().await;
        assert_eq!(
            *provider.efforts.lock().unwrap(),
            [Some(Effort::High), Some(Effort::Low), Some(Effort::High)]
        );
    }

    /// A switch of mode applies from the next turn: the new prompt leads the request.
    #[tokio::test]
    async fn the_next_turn_takes_up_a_new_mode() {
        let state = thread_state("mode_switch").await;
        let config = live_config("mode_switch_config");
        let provider = provider(vec![
            vec![Step::Text("one"), Step::Done],
            vec![Step::Text("two"), Step::Done],
        ]);
        let (runner, input, _output) = runner(&state, &provider, &config, model(None));
        let modes = runner.agent.modes_dir.clone();
        input.send("first").await.unwrap();
        let running = tokio::spawn(runner.run());
        while provider.requests.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        std::fs::write(modes.join("PLAN.md"), "Plan only.").unwrap();
        config
            .update(|config| config.set_mode("PLAN".into()))
            .unwrap();
        input.send("second").await.unwrap();
        drop(input);
        running.await.unwrap();

        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests[0][0], ChatMessage::System(MAIN_PROMPT.into()));
        assert_eq!(
            requests.last().unwrap()[0],
            ChatMessage::System("Plan only.".into())
        );
    }

    #[tokio::test]
    async fn a_passing_failure_is_retried() {
        let state = thread_state("retry").await;
        let config = live_config("retry_config");
        let provider = provider(vec![vec![Step::Fail], vec![Step::Text("ok"), Step::Done]]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("hello").await.unwrap();
        drop(input);

        runner.run().await;

        let mut retried = false;
        let mut status = None;
        while let Some(event) = output.next_event().await {
            match event {
                ThreadEvent::Retrying { attempt, error } => {
                    retried = attempt == 1 && error == "server busy";
                }
                ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
                _ => {}
            }
        }
        assert!(retried);
        assert_eq!(status, Some(TurnStatus::Completed));
    }
}
