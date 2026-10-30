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

use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, ensure};
use tokio::sync::mpsc;

use super::{ThreadLock, model_choice::ModelChoice};
use crate::runtime::agent::{Agent, Inbox, Input, Mailbox, Mailboxes, Stop};
use crate::{
    chat::commands,
    live_config::LiveConfig,
    providers::{ModelEvent, ToolCall},
    state::{
        Compaction, LocalUnsent, Message, PendingMessage, State, Task, ThreadTokens, Turn,
        estimate_tokens,
    },
    tools::{Question, ToolRegistry, ToolResult, ToolScope},
};

/// Runs one thread; `Runtime` gives each active thread its own task with one of these.
pub struct ThreadRunner {
    agent: Agent,
    inbox: Inbox,
    mailboxes: Arc<Mailboxes>,
    ends_when_idle: bool,
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
    pub scope: ToolScope,
    pub compact_prompt: PathBuf,
    pub fork_prompt: PathBuf,
    pub modes_dir: PathBuf,
    /// How many agents are above this thread; 0 for a chat.
    pub depth: usize,
    pub mailboxes: Arc<Mailboxes>,
    /// A subagent's runner ends when idle instead of waiting for its client.
    pub ends_when_idle: bool,
    pub lock: ThreadLock,
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

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreadEvent {
    /// The active model changed or was discovered after the thread opened.
    ModelSelected {
        name: String,
        context_window: Option<u64>,
    },
    /// Saved locally, outside the model queue, until the client explicitly resends it.
    LocalUnsent(LocalUnsent),
    /// The selected provider could not answer, or no model is selected.
    ProviderUnavailable,
    Queued {
        message_id: String,
    },
    Rejected {
        reason: String,
    },
    TurnStarted(Turn),
    /// A model call failed in a way that may pass; the next attempt follows after a pause.
    Retrying {
        attempt: u32,
        error: String,
    },
    /// Everything the model streams: reasoning, text, tool calls, usage.
    Model(ModelEvent),
    ToolStarted(ToolCall),
    ToolFinished {
        call_id: String,
        result: ToolResult,
    },
    TurnFinished(Turn),
    /// The thread got a name: the start of its first message at once, then one the model
    /// gives after the first turn.
    TitleChanged {
        title: String,
    },
    /// The model wrote `limit` tokens in one call and was stopped; the turn ends with
    /// what it wrote.
    OutputLimitReached {
        limit: u64,
    },
    /// The model's answer broke off, such as on a dropped connection; the turn ends with
    /// what it wrote.
    AnswerCut {
        error: String,
    },
    /// The model is summarizing the context; its text streams as `Model` events.
    CompactionStarted,
    CompactionFinished(Compaction),
    /// The context stays as it was.
    CompactionFailed {
        error: String,
    },
    /// The thread's token counts, when it starts and after each turn and compaction. `limit` is the largest request: the
    /// configured input limit or the model window, whichever is smaller.
    Tokens {
        tokens: ThreadTokens,
        limit: u64,
    },
    /// The context is past the input limit, so the queued turn was not started or the
    /// running one ended; the client should switch model or compact.
    ContextTooLarge {
        tokens: u64,
        limit: u64,
    },
    /// The tool call `call_id` gave `task` to `agent`, which works in thread `thread_id`.
    SubagentStarted {
        call_id: String,
        thread_id: String,
        agent: String,
        task: String,
    },
    /// An event of a subagent's thread. A subagent's own subagents come wrapped once more.
    Subagent {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// The thread's task board changed: every task with its steps.
    Tasks(Vec<Task>),
    /// The agent asks the user to choose; the answer comes as the user's next message.
    Question(Question),
    /// Another agent wrote to this thread; the message has joined it.
    FromAgent(Message),
    /// /fork wrote its summary; a client opens a new thread that starts from it.
    Forked {
        summary: String,
    },
    /// Nothing is running and nothing will start until the next message.
    Idle,
    /// The thread stopped on an error it could not handle, such as a database failure.
    Failed(String),
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
            thread_id: setup.thread_id,
            compact_prompt: setup.compact_prompt,
            fork_prompt: setup.fork_prompt,
            modes_dir: setup.modes_dir,
            depth: setup.depth,
            events,
        };
        let runner = Self {
            agent,
            inbox: Inbox::new(inputs),
            mailboxes: setup.mailboxes,
            ends_when_idle: setup.ends_when_idle,
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
        // The client sees the context size before the first turn, too.
        self.agent.report_tokens().await;
        loop {
            if self.agent.model.current().is_none() {
                self.agent.emit(ThreadEvent::Idle);
                if self.ends_when_idle {
                    return Ok(());
                }
                match self.inbox.wait().await {
                    None => return Ok(()),
                    Some(Input::Cancel) => self.cancel_agents().await?,
                    Some(_) => {}
                }
                continue;
            }
            self.take_in().await?;
            self.run_queued().await?;
            if self.has_pending().await? {
                continue;
            }
            self.agent.emit(ThreadEvent::Idle);
            if self.ends_when_idle {
                if self.leave_if_idle().await? {
                    return Ok(());
                }
                continue;
            }
            match self.inbox.wait().await {
                None => return Ok(()),
                // The agents this thread asked may still be at work.
                Some(Input::Cancel) => self.cancel_agents().await?,
                Some(_) => {}
            }
        }
    }

    /// Runs turns until nothing is queued, the user cancels or the context does not fit.
    /// The context is compacted first when it has grown too large. What was sent
    /// meanwhile is taken in after each turn, so the next one answers it.
    async fn run_queued(&mut self) -> Result<()> {
        loop {
            let stop = match self.agent.compact_if_needed(None, &mut self.inbox).await {
                Some(stop) => Some(stop),
                None => match self.agent.start_turn().await? {
                    Some(turn) => match self.agent.run_turn(&turn, &mut self.inbox).await? {
                        Some(stop) => Some(stop),
                        None => self.agent.name_thread(&mut self.inbox).await,
                    },
                    None => return Ok(()),
                },
            };
            self.take_in().await?;
            if stop == Some(Stop::Cancelled) {
                return self.cancel_agents().await;
            }
        }
    }

    /// Takes in what clients sent, oldest first: messages join the thread, commands run.
    /// A command can take a while, so whatever arrived meanwhile is taken in as well.
    async fn take_in(&mut self) -> Result<()> {
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
        let command = match pending.from_thread_id {
            Some(_) => None,
            None => commands::parse(&pending.content),
        };
        let Some(command) = command else {
            let message = state.admit_pending(&self.agent.thread_id, &pending).await?;
            self.agent.title_from_first_message(&message).await?;
            self.agent.emit(ThreadEvent::Queued {
                message_id: message.message_id.clone(),
            });
            if message.from_thread_id.is_some() {
                self.agent.emit(ThreadEvent::FromAgent(message));
            }
            return Ok(());
        };
        match command {
            Err(error) => self.agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            }),
            Ok(command) => command.run_in_thread(&self.agent, &mut self.inbox).await,
        }
        // Removed only once done: a command cut short by a crash runs again on restart.
        state.remove_pending(pending.pending_id).await
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
    pub fn cancel(&self) -> Result<()> {
        ensure!(self.inputs.send(Input::Cancel).is_ok(), "thread has ended");
        Ok(())
    }

    async fn deliver(&self, text: String, input: Input) -> Result<()> {
        ensure!(!self.inputs.is_closed(), "thread has ended");
        // A message that alone is past the input limit could never be answered.
        let limit = self.config.current().limits().max_input_tokens;
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

    /// An event that has already arrived, without waiting.
    pub fn ready_event(&mut self) -> Option<ThreadEvent> {
        self.events.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use serde_json::{Value, json};

    use super::*;
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
    }

    #[derive(Clone, Copy)]
    enum Step {
        Text(&'static str),
        /// Calls the `echo` tool with this text.
        Call(&'static str),
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

        fn description(&self, _context: &ToolContext<'_>) -> String {
            "Returns its text.".into()
        }

        fn parameters(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call<'a>(
            &'a self,
            arguments: Value,
            _context: ToolContext<'a>,
        ) -> BoxFuture<'a, String> {
            Box::pin(async move { Ok(arguments["text"].as_str().unwrap_or_default().to_owned()) })
        }
    }

    fn provider(scripts: Vec<Vec<Step>>) -> Arc<FakeProvider> {
        Arc::new(FakeProvider {
            scripts: Mutex::new(scripts.into()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn model(context_window: Option<u64>) -> Model {
        Model {
            id: "fake".into(),
            context_window,
        }
    }

    async fn thread_state(name: &str) -> Arc<State> {
        let state = Arc::new(temp_state(name).await);
        state.create_thread("thread", "test").await.unwrap();
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
        let choice = Choice {
            provider_name: "fake".into(),
            provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
            model,
        };
        ThreadRunner::new(ThreadSetup {
            state: Arc::clone(state),
            model: Arc::new(ModelChoice::new(choice)),
            config: Arc::clone(config),
            thread_id: "thread".into(),
            tools,
            scope: ToolScope::new(&root, &root).unwrap(),
            compact_prompt: root.join("COMPACT.md"),
            fork_prompt: root.join("FORK.md"),
            modes_dir: root.join("modes"),
            depth: 0,
            mailboxes: Arc::default(),
            ends_when_idle: false,
            lock: ThreadLock::take(&root).unwrap(),
        })
    }

    async fn next_turn_finished(output: &mut ThreadOutput) -> Turn {
        loop {
            if let Some(ThreadEvent::TurnFinished(turn)) = output.next_event().await {
                return turn;
            }
        }
    }

    async fn transcript(state: &State) -> Vec<(MessageRole, String)> {
        let messages = state.list_messages("thread").await.unwrap();
        messages
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect()
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
            input.cancel().unwrap();
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
        let limits = "limits:\n  max_output_tokens: 2\n";
        let config = Arc::new(temp_live_config_with("output_limit_config", limits));
        // The third piece takes the output to 3 tokens; the stream must not be read on.
        let script = vec![
            Step::Text("abcd"),
            Step::Text("efgh"),
            Step::Text("ijkl"),
            Step::Hang,
        ];
        let provider = provider(vec![script]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("write a lot").await.unwrap();
        drop(input);

        runner.run().await;

        let mut reached = None;
        let mut status = None;
        while let Some(event) = output.next_event().await {
            match event {
                ThreadEvent::OutputLimitReached { limit } => reached = Some(limit),
                ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
                _ => {}
            }
        }
        assert_eq!(reached, Some(2));
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
        let long = "a tool result long enough to push the context past the limit";
        let provider = provider(vec![
            vec![Step::Call(long), Step::Done],
            vec![Step::Text("used the tool"), Step::Done],
            vec![Step::Text("done"), Step::Done],
        ]);
        // 16 tokens before the step and about 60 after it: past 80% of 60, within 60
        // only after the summary.
        let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(60)));
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
        assert_eq!(requests[2].len(), 1, "only the summary is left");
    }

    /// Like a subagent writing back while the asking turn runs a tool.
    struct WriteBack(Arc<State>);

    impl Tool for WriteBack {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn description(&self, _context: &ToolContext<'_>) -> String {
            "Lets the subagent write back.".into()
        }

        fn parameters(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call<'a>(
            &'a self,
            _arguments: Value,
            _context: ToolContext<'a>,
        ) -> BoxFuture<'a, String> {
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
