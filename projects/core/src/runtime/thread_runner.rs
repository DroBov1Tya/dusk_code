//! A live thread. Messages can be sent at any time; the agent runs one turn at a time
//! over everything queued. `send_now` stops the running turn, keeping what it wrote,
//! so the next turn starts at once with the new message.
//!
//! Messages sent during a turn are held in memory and stored right after the turn ends,
//! so the thread always reads in the order things happened.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    agent::Agent,
    inbox::{Command, Inbox, Stop},
};
use crate::{
    live_config::LiveConfig,
    providers::{Model, ModelEvent, ModelProvider, ToolCall},
    state::{State, Turn},
    tools::{ToolRegistry, ToolResult, ToolScope},
};

pub struct ThreadRunner {
    agent: Agent,
    inbox: Inbox,
}

/// Everything a running thread needs; the thread must exist.
pub struct ThreadSetup {
    pub state: Arc<State>,
    pub provider: Arc<dyn ModelProvider>,
    pub model: Model,
    pub config: Arc<LiveConfig>,
    pub thread_id: String,
    pub tools: ToolRegistry,
    pub scope: ToolScope,
}

/// The client's side for sending. Dropping every clone ends the thread once the queue is done.
#[derive(Clone)]
pub struct ThreadInput {
    commands: mpsc::UnboundedSender<Command>,
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
    /// Everything the model streams: reasoning, text, tool calls, usage.
    Model(ModelEvent),
    ToolStarted(ToolCall),
    ToolFinished {
        call_id: String,
        result: ToolResult,
    },
    TurnFinished(Turn),
    /// The queued turn was not started; the client should switch model or compact.
    ContextTooLarge {
        tokens: u64,
        limit: u64,
    },
    /// Nothing is running and nothing will start until the next message.
    Idle,
}

impl ThreadRunner {
    pub fn new(setup: ThreadSetup) -> (Self, ThreadInput, ThreadOutput) {
        let (command_sender, commands) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let agent = Agent {
            state: setup.state,
            provider: setup.provider,
            model: setup.model,
            config: setup.config,
            tools: setup.tools,
            scope: setup.scope,
            thread_id: setup.thread_id,
            events,
        };
        let runner = Self {
            agent,
            inbox: Inbox::new(commands),
        };
        let input = ThreadInput {
            commands: command_sender,
        };
        let output = ThreadOutput {
            events: event_receiver,
        };
        (runner, input, output)
    }

    /// Runs until every `ThreadInput` is dropped and the queue is done.
    pub async fn run(mut self) -> Result<()> {
        self.agent.recover().await?;
        loop {
            self.run_queued().await?;
            self.agent.emit(ThreadEvent::Idle);
            let Some(command) = self.inbox.next().await else {
                return Ok(());
            };
            self.accept(command).await;
            // Everything already sent goes into the same turn.
            for command in self.inbox.ready() {
                self.accept(command).await;
            }
        }
    }

    /// Runs turns until nothing is queued, the user cancels or the context does not fit.
    async fn run_queued(&mut self) -> Result<()> {
        while let Some(turn) = self.agent.start_turn().await? {
            let stop = self.agent.run_turn(&turn, &mut self.inbox).await?;
            for text in self.inbox.take_held() {
                self.store(text).await;
            }
            if stop == Some(Stop::Cancelled) {
                return Ok(());
            }
        }
        Ok(())
    }

    async fn accept(&self, command: Command) {
        match command {
            Command::Send(text) | Command::SendNow(text) => self.store(text).await,
            Command::Cancel => {}
        }
    }

    async fn store(&self, text: String) {
        let request_uid = Uuid::now_v7().to_string();
        let stored = self
            .agent
            .state
            .add_user_message(&self.agent.thread_id, &request_uid, &text)
            .await;
        let event = match stored {
            Ok(message) => ThreadEvent::Queued {
                message_id: message.message_id,
            },
            Err(error) => ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            },
        };
        self.agent.emit(event);
    }
}

impl ThreadInput {
    /// Queues a message; it is answered after the running turn, together with other queued ones.
    pub fn send(&self, text: impl Into<String>) -> Result<()> {
        self.command(Command::Send(text.into()))
    }

    /// Queues a message and stops the running turn, so the agent starts on it at once.
    pub fn send_now(&self, text: impl Into<String>) -> Result<()> {
        self.command(Command::SendNow(text.into()))
    }

    /// Stops the running turn; queued messages wait for the next one.
    pub fn cancel(&self) -> Result<()> {
        self.command(Command::Cancel)
    }

    fn command(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| anyhow!("thread has ended"))
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
        providers::{ChatRequest, ChatStream, FinishReason},
        state::{MessageRole, TurnStatus},
        testing::{temp_dir, temp_live_config, temp_state},
        tools::{Tool, ToolContext},
    };

    /// Each model call takes the next script; `Hang` never finishes, like a slow model.
    struct FakeProvider {
        scripts: Mutex<VecDeque<Vec<Step>>>,
    }

    #[derive(Clone, Copy)]
    enum Step {
        Text(&'static str),
        /// Calls the `echo` tool with this text.
        Call(&'static str),
        Hang,
        Done,
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
            _request: ChatRequest<'a>,
        ) -> BoxFuture<'a, Box<dyn ChatStream>> {
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
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
                    None => None,
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
        input.send("a").unwrap();
        input.send("b").unwrap();
        drop(input);

        runner.run().await.unwrap();

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
            input.send("first").unwrap();
            while output.next_event().await
                != Some(ThreadEvent::Model(ModelEvent::Text("partial".into())))
            {}
            input.send_now("now").unwrap();
            let first = next_turn_finished(&mut output).await;
            let second = next_turn_finished(&mut output).await;
            (first.status, second.status)
        };

        let (result, statuses) = tokio::join!(runner.run(), client);

        result.unwrap();
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
            input.send("first").unwrap();
            while !matches!(output.next_event().await, Some(ThreadEvent::TurnStarted(_))) {}
            input.cancel().unwrap();
            let turn = next_turn_finished(&mut output).await;
            assert_eq!(output.next_event().await, Some(ThreadEvent::Idle));
            turn.status
        };

        let (result, status) = tokio::join!(runner.run(), client);

        result.unwrap();
        assert_eq!(status, TurnStatus::Interrupted);
        assert_eq!(transcript(&state).await.len(), 1, "nothing was written");
    }

    #[tokio::test]
    async fn oversized_context_is_reported_and_not_sent() {
        let state = thread_state("too_large").await;
        let config = live_config("runner_config");
        let provider = provider(Vec::new());
        let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(3)));
        input.send("hello").unwrap();
        drop(input);

        runner.run().await.unwrap();

        let mut events = Vec::new();
        while let Some(event) = output.next_event().await {
            events.push(event);
        }
        assert!(events.contains(&ThreadEvent::ContextTooLarge {
            tokens: 6,
            limit: 3
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ThreadEvent::TurnStarted(_)))
        );
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
        input.send("use the tool").unwrap();
        drop(input);

        runner.run().await.unwrap();

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
        input.send("use the tool").unwrap();
        drop(input);

        runner.run().await.unwrap();

        let messages = transcript(&state).await;
        assert_eq!(
            messages[2],
            (MessageRole::Tool, "error: tool echo is switched off".into())
        );
    }
}
