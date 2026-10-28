//! A live thread. Messages can be sent at any time; the agent runs one turn at a time
//! over everything queued. `send_now` stops the running turn, keeping what it wrote,
//! so the next turn starts at once with the new message.
//!
//! A send is confirmed once the message is stored as pending. The thread takes pending
//! messages in when it is free: right away when idle, after the running turn otherwise,
//! so the thread always reads in the order things happened. A message that is a command,
//! such as `/compact`, runs at that point instead of joining the thread.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, ensure};
use tokio::sync::mpsc;

use super::{
    agent::Agent,
    inbox::{Inbox, Input, Stop},
};
use crate::{
    commands::{self, Command},
    live_config::LiveConfig,
    providers::{Model, ModelEvent, ModelProvider, ToolCall},
    state::{
        Compaction, CompactionReason, PendingMessage, State, ThreadTokens, Turn, estimate_tokens,
    },
    tools::{ToolRegistry, ToolResult, ToolScope},
};

/// Runs one thread; `Runtime` gives each active thread its own task with one of these.
pub(super) struct ThreadRunner {
    agent: Agent,
    inbox: Inbox,
}

/// Everything a running thread needs; the thread must exist.
pub(super) struct ThreadSetup {
    pub state: Arc<State>,
    pub provider: Arc<dyn ModelProvider>,
    pub model: Model,
    pub config: Arc<LiveConfig>,
    pub thread_id: String,
    pub tools: ToolRegistry,
    pub scope: ToolScope,
    pub compact_prompt: PathBuf,
}

/// The client's side for sending. Dropping every clone ends the thread once the queue is done.
#[derive(Clone)]
pub struct ThreadInput {
    state: Arc<State>,
    config: Arc<LiveConfig>,
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
    /// The model is summarizing the context; its text streams as `Model` events.
    CompactionStarted {
        reason: CompactionReason,
    },
    CompactionFinished(Compaction),
    /// The context stays as it was.
    CompactionFailed {
        error: String,
    },
    /// The thread's token counts, after each turn. `limit` is the largest request: the
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
    /// Nothing is running and nothing will start until the next message.
    Idle,
    /// The thread stopped on an error it could not handle, such as a database failure.
    Failed(String),
}

impl ThreadRunner {
    pub(super) fn new(setup: ThreadSetup) -> (Self, ThreadInput, ThreadOutput) {
        let (input_sender, inputs) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let input = ThreadInput {
            state: Arc::clone(&setup.state),
            config: Arc::clone(&setup.config),
            thread_id: setup.thread_id.clone(),
            inputs: input_sender,
        };
        let agent = Agent {
            state: setup.state,
            provider: setup.provider,
            model: setup.model,
            config: setup.config,
            tools: setup.tools,
            scope: setup.scope,
            thread_id: setup.thread_id,
            compact_prompt: setup.compact_prompt,
            events,
        };
        let runner = Self {
            agent,
            inbox: Inbox::new(inputs),
        };
        let output = ThreadOutput {
            events: event_receiver,
        };
        (runner, input, output)
    }

    /// Runs until every `ThreadInput` is dropped and the queue is done. An error ends the
    /// thread and is reported to the client as `Failed`.
    pub(super) async fn run(mut self) {
        if let Err(error) = self.serve().await {
            self.agent.emit(ThreadEvent::Failed(format!("{error:#}")));
        }
    }

    async fn serve(&mut self) -> Result<()> {
        self.agent.recover().await?;
        loop {
            self.take_in().await?;
            self.run_queued().await?;
            if self.has_pending().await? {
                continue;
            }
            self.agent.emit(ThreadEvent::Idle);
            if !self.inbox.wait().await {
                return Ok(());
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
                return Ok(());
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
        let Some(command) = commands::parse(&pending.content) else {
            let message = state.admit_pending(&self.agent.thread_id, &pending).await?;
            self.agent.title_from_first_message(&message).await?;
            self.agent.emit(ThreadEvent::Queued {
                message_id: message.message_id,
            });
            return Ok(());
        };
        match command {
            Err(error) => self.agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            }),
            Ok(Command::Compact { focus }) => {
                let reason = CompactionReason::Manual;
                // A stop only ends the compaction; the client decides what comes next.
                let _ = self
                    .agent
                    .compact(reason, focus.as_deref(), None, &mut self.inbox)
                    .await;
            }
        }
        // Removed only once done: a command cut short by a crash runs again on restart.
        state.remove_pending(pending.pending_id).await
    }

    async fn has_pending(&self) -> Result<bool> {
        let pending = self
            .agent
            .state
            .pending_messages(&self.agent.thread_id)
            .await?;
        Ok(!pending.is_empty())
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
    use crate::{
        BoxFuture,
        providers::{ChatMessage, ChatRequest, ChatStream, FinishReason, Transient},
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
        let root = temp_dir("runner_scope");
        std::fs::create_dir_all(&root).unwrap();
        let mut tools = ToolRegistry::default();
        tools.add(Box::new(Echo));
        ThreadRunner::new(ThreadSetup {
            state: Arc::clone(state),
            provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
            model,
            config: Arc::clone(config),
            thread_id: "thread".into(),
            tools,
            scope: ToolScope::new(&root, &root).unwrap(),
            compact_prompt: root.join("COMPACT.md"),
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
    async fn messages_queued_before_a_turn_are_answered_together() {
        let state = thread_state("together").await;
        let config = live_config("runner_config");
        let provider = provider(vec![vec![Step::Text("both"), Step::Done]]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("a").await.unwrap();
        input.send("b").await.unwrap();
        drop(input);

        runner.run().await;

        let turn = next_turn_finished(&mut output).await;
        assert_eq!(turn.status, TurnStatus::Completed);
        assert_eq!(turn.last_sequence, 2);
        assert_eq!(
            transcript(&state).await,
            [
                (MessageRole::User, "a".into()),
                (MessageRole::User, "b".into()),
                (MessageRole::Assistant, "both".into()),
            ]
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

        let ((), statuses) = tokio::join!(runner.run(), client);
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

        let ((), status) = tokio::join!(runner.run(), client);
        assert_eq!(status, TurnStatus::Interrupted);
        assert_eq!(transcript(&state).await.len(), 1, "nothing was written");
    }

    #[tokio::test]
    async fn oversized_context_is_reported_and_not_sent() {
        let state = thread_state("too_large").await;
        let config = live_config("runner_config");
        let provider = provider(Vec::new());
        let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(3)));
        input.send("hello").await.unwrap();
        drop(input);

        runner.run().await;

        let mut events = Vec::new();
        while let Some(event) = output.next_event().await {
            events.push(event);
        }
        // The message is 6 and the echo tool definition 1 + 5 + 5.
        assert!(events.contains(&ThreadEvent::ContextTooLarge {
            tokens: 17,
            limit: 3
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ThreadEvent::TurnStarted(_)))
        );
    }

    async fn next_compaction(output: &mut ThreadOutput) -> Compaction {
        loop {
            match output.next_event().await {
                Some(ThreadEvent::CompactionFinished(compaction)) => return compaction,
                Some(ThreadEvent::CompactionFailed { error }) => panic!("{error}"),
                Some(_) => {}
                None => panic!("the thread ended"),
            }
        }
    }

    #[tokio::test]
    async fn compact_summarizes_and_later_turns_see_the_summary() {
        let state = thread_state("compact_manual").await;
        let config = live_config("runner_config");
        let provider = provider(vec![
            vec![Step::Text("one"), Step::Done],
            vec![Step::Text("Numbers"), Step::Done],
            vec![Step::Text("so far: one"), Step::Done],
            vec![Step::Text("two"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            input.send("first").await.unwrap();
            next_turn_finished(&mut output).await;
            input.send("/compact the numbers").await.unwrap();
            let compaction = next_compaction(&mut output).await;
            input.send("second").await.unwrap();
            next_turn_finished(&mut output).await;
            compaction
        };

        let ((), compaction) = tokio::join!(runner.run(), client);

        assert_eq!(compaction.through_sequence, 2);
        assert_eq!(compaction.reason, CompactionReason::Manual);
        let requests = provider.requests.lock().unwrap().clone();
        // 0 is the first turn and 1 names the thread.
        let instructions = requests[2].last().unwrap();
        let ChatMessage::User(instructions) = instructions else {
            panic!("{instructions:?}");
        };
        assert!(instructions.ends_with("Focus the summary on: the numbers\n"));
        assert_eq!(requests[3].len(), 2, "summary and the new message");
        assert!(
            matches!(&requests[3][0], ChatMessage::User(text) if text.ends_with("so far: one"))
        );
        assert_eq!(requests[3][1], ChatMessage::User("second".into()));
        let stored = transcript(&state).await;
        assert_eq!(stored.len(), 4, "messages stay stored: {stored:?}");
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
    async fn a_message_past_the_input_limit_is_refused_at_once() {
        let state = thread_state("input_limit").await;
        let limits = "limits:\n  max_input_tokens: 10\n";
        let config = Arc::new(temp_live_config_with("input_limit_config", limits));
        let provider = provider(Vec::new());
        let (_runner, input, _output) = runner(&state, &provider, &config, model(None));

        let error = input.send("x".repeat(100)).await.unwrap_err();

        assert!(
            error.to_string().contains("past the input limit of 10"),
            "{error}"
        );
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_first_message_names_the_thread_and_the_model_renames_it() {
        let state = thread_state("title").await;
        let config = live_config("runner_config");
        let provider = provider(vec![
            vec![Step::Text("Hello!"), Step::Done],
            vec![Step::Text("\"Greeting\""), Step::Done],
            vec![Step::Text("Fine."), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        let client = async move {
            let mut titles = Vec::new();
            input
                .send("hi there, how are you doing today, my friend?")
                .await
                .unwrap();
            input.send("and now?").await.unwrap();
            drop(input);
            while let Some(event) = output.next_event().await {
                if let ThreadEvent::TitleChanged { title } = event {
                    titles.push(title);
                }
            }
            titles
        };

        let ((), titles) = tokio::join!(runner.run(), client);

        assert_eq!(
            titles,
            ["hi there, how are you doing today,...", "Greeting"]
        );
        let thread = state.load_thread("thread").await.unwrap().unwrap();
        assert_eq!(thread.title.as_deref(), Some("Greeting"));
        let requests = provider.requests.lock().unwrap().clone();
        assert_eq!(
            requests.len(),
            2,
            "both messages in one turn, then the name"
        );
    }

    #[tokio::test]
    async fn an_unknown_command_is_rejected_and_not_stored() {
        let state = thread_state("compact_unknown").await;
        let config = live_config("runner_config");
        let provider = provider(Vec::new());
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("/compactt").await.unwrap();
        drop(input);

        runner.run().await;

        let mut rejected = None;
        while let Some(event) = output.next_event().await {
            if let ThreadEvent::Rejected { reason } = event {
                rejected = Some(reason);
            }
        }
        assert_eq!(rejected.as_deref(), Some("unknown command /compactt"));
        assert!(transcript(&state).await.is_empty());
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
        assert_eq!(compaction.unwrap().reason, CompactionReason::Auto);
        assert_eq!(status, Some(TurnStatus::Completed));
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests[2].len(), 1, "only the summary is left");
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
    async fn a_switched_off_tool_is_refused() {
        let state = thread_state("tool_off").await;
        let config = live_config("tool_off_config");
        config
            .update(|config| config.set_tool_enabled("echo", false))
            .unwrap();
        let provider = provider(vec![
            vec![Step::Call("ping"), Step::Done],
            vec![Step::Text("ok"), Step::Done],
        ]);
        let (runner, input, _output) = runner(&state, &provider, &config, model(None));
        input.send("use the tool").await.unwrap();
        drop(input);

        runner.run().await;

        let messages = transcript(&state).await;
        assert_eq!(
            messages[2],
            (MessageRole::Tool, "error: tool echo is switched off".into())
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
