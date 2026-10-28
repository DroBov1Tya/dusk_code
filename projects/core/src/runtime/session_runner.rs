//! A live chat session. Messages can be sent at any time; the agent runs one turn at a
//! time over everything queued. `send_now` stops the running turn, keeping what it wrote,
//! so the next turn starts at once with the new message.
//!
//! Messages sent during a turn are held in memory and stored right after the turn ends,
//! so the thread always reads in the order things happened.

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    agent::Agent,
    inbox::{Command, Inbox, Stop},
};
use crate::{
    config::RecordMode,
    providers::{Model, ModelEvent, ModelProvider, ToolCall},
    receivers::{AgentReceiver, UserMessage, UserReceiver},
    state::{State, Turn},
    tools::{ToolRegistry, ToolResult, ToolScope},
};

pub struct SessionRunner<'a, P> {
    agent: Agent<'a, P>,
    users: UserReceiver<'a>,
    inbox: Inbox,
    cwd: String,
}

/// Everything a session needs; the thread must exist.
pub struct SessionSetup<'a, P> {
    pub state: &'a State,
    pub provider: &'a P,
    pub model: Model,
    pub record: RecordMode,
    pub agent_id: &'a str,
    pub thread_id: &'a str,
    pub tools: ToolRegistry,
    pub scope: ToolScope,
}

/// The client's side for sending. Dropping every clone ends the session once the queue is done.
#[derive(Clone)]
pub struct SessionInput {
    commands: mpsc::UnboundedSender<Command>,
}

/// The client's side for receiving.
pub struct SessionOutput {
    events: mpsc::UnboundedReceiver<SessionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEvent {
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

impl<'a, P: ModelProvider> SessionRunner<'a, P> {
    pub fn new(setup: SessionSetup<'a, P>) -> Result<(Self, SessionInput, SessionOutput)> {
        let (command_sender, commands) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let cwd = setup.scope.cwd().to_string_lossy().into_owned();
        let agent = Agent {
            receiver: AgentReceiver::new(setup.state, setup.agent_id)?,
            provider: setup.provider,
            model: setup.model,
            record: setup.record,
            tools: setup.tools,
            scope: setup.scope,
            thread_id: setup.thread_id.to_owned(),
            events,
        };
        let runner = Self {
            agent,
            users: UserReceiver::new(setup.state),
            inbox: Inbox::new(commands),
            cwd,
        };
        let input = SessionInput {
            commands: command_sender,
        };
        let output = SessionOutput {
            events: event_receiver,
        };
        Ok((runner, input, output))
    }

    /// Runs until every `SessionInput` is dropped and the queue is done.
    pub async fn run(mut self) -> Result<()> {
        self.agent.recover().await?;
        loop {
            self.run_queued().await?;
            self.agent.emit(SessionEvent::Idle);
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
        let message = UserMessage {
            request_uid: Uuid::now_v7().to_string(),
            thread_id: self.agent.thread_id.clone(),
            content: text,
            cwd: self.cwd.clone(),
            attachments: Vec::new(),
        };
        let event = match self.users.submit_message(message).await {
            Ok(receipt) => match (receipt.message, receipt.execution.error_code) {
                (Some(message), _) => SessionEvent::Queued {
                    message_id: message.message_id,
                },
                (None, code) => SessionEvent::Rejected {
                    reason: code.map_or("rejected", |code| code.as_str()).to_owned(),
                },
            },
            Err(error) => SessionEvent::Rejected {
                reason: format!("{error:#}"),
            },
        };
        self.agent.emit(event);
    }
}

impl SessionInput {
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
            .map_err(|_| anyhow!("session has ended"))
    }
}

impl SessionOutput {
    /// `None` once the session has ended.
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        self.events.recv().await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use serde_json::{Value, json};

    use super::*;
    use crate::{
        providers::{ChatRequest, ChatStream, FinishReason},
        state::{ApprovalMode, MessageRole, ThreadInput, TurnStatus},
        testing::{temp_dir, temp_state},
        tools::{Tool, ToolFuture},
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
        type Stream = FakeStream;

        async fn list_models(&self) -> Result<Vec<Model>> {
            Ok(Vec::new())
        }

        async fn stream_chat(&self, _request: ChatRequest<'_>) -> Result<FakeStream> {
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Ok(FakeStream {
                steps: script.into(),
                pending: VecDeque::new(),
            })
        }
    }

    impl ChatStream for FakeStream {
        async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
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
        }
    }

    impl Tool for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn description(&self, _scope: &ToolScope) -> String {
            "Returns its text.".into()
        }

        fn parameters(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call<'a>(&'a self, arguments: Value, _scope: &'a ToolScope) -> ToolFuture<'a> {
            Box::pin(async move { Ok(arguments["text"].as_str().unwrap_or_default().to_owned()) })
        }
    }

    fn provider(scripts: Vec<Vec<Step>>) -> FakeProvider {
        FakeProvider {
            scripts: Mutex::new(scripts.into()),
        }
    }

    fn model(context_window: Option<u64>) -> Model {
        Model {
            id: "fake".into(),
            context_window,
        }
    }

    async fn session_state(name: &str) -> State {
        let state = temp_state(name).await;
        let thread = ThreadInput {
            source: "test".into(),
            tokens_used: 0,
            compaction_count: 0,
            sandbox_policy: "none".into(),
            approval_mode: ApprovalMode::Never,
            git_branch: None,
            git_commit_sha: None,
            git_origin_url: None,
            title: String::new(),
        };
        state.create_thread("thread", thread).await.unwrap();
        state
    }

    fn runner<'a>(
        state: &'a State,
        provider: &'a FakeProvider,
        model: Model,
    ) -> (SessionRunner<'a, FakeProvider>, SessionInput, SessionOutput) {
        let root = temp_dir("runner_scope");
        std::fs::create_dir_all(&root).unwrap();
        let mut tools = ToolRegistry::default();
        tools.add(Box::new(Echo));
        SessionRunner::new(SessionSetup {
            state,
            provider,
            model,
            record: RecordMode::Full,
            agent_id: "agent",
            thread_id: "thread",
            tools,
            scope: ToolScope::new(&root, &root).unwrap(),
        })
        .unwrap()
    }

    async fn next_turn_finished(output: &mut SessionOutput) -> Turn {
        loop {
            if let Some(SessionEvent::TurnFinished(turn)) = output.next_event().await {
                return turn;
            }
        }
    }

    async fn transcript(state: &State) -> Vec<(MessageRole, String)> {
        let messages = state.list_messages("thread", 0, 100).await.unwrap();
        messages
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect()
    }

    #[tokio::test]
    async fn messages_queued_before_a_turn_are_answered_together() {
        let state = session_state("together").await;
        let provider = provider(vec![vec![Step::Text("both"), Step::Done]]);
        let (runner, input, mut output) = runner(&state, &provider, model(None));
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
        let state = session_state("send_now").await;
        let provider = provider(vec![
            vec![Step::Text("partial"), Step::Hang],
            vec![Step::Text("second"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, model(None));
        let client = async move {
            input.send("first").unwrap();
            while output.next_event().await
                != Some(SessionEvent::Model(ModelEvent::Text("partial".into())))
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
        let state = session_state("cancel").await;
        let provider = provider(vec![vec![Step::Hang]]);
        let (runner, input, mut output) = runner(&state, &provider, model(None));
        let client = async move {
            input.send("first").unwrap();
            while !matches!(
                output.next_event().await,
                Some(SessionEvent::TurnStarted(_))
            ) {}
            input.cancel().unwrap();
            let turn = next_turn_finished(&mut output).await;
            assert_eq!(output.next_event().await, Some(SessionEvent::Idle));
            turn.status
        };

        let (result, status) = tokio::join!(runner.run(), client);

        result.unwrap();
        assert_eq!(status, TurnStatus::Interrupted);
        assert_eq!(transcript(&state).await.len(), 1, "nothing was written");
    }

    #[tokio::test]
    async fn oversized_context_is_reported_and_not_sent() {
        let state = session_state("too_large").await;
        let provider = provider(Vec::new());
        let (runner, input, mut output) = runner(&state, &provider, model(Some(3)));
        input.send("hello").unwrap();
        drop(input);

        runner.run().await.unwrap();

        let mut events = Vec::new();
        while let Some(event) = output.next_event().await {
            events.push(event);
        }
        assert!(events.contains(&SessionEvent::ContextTooLarge {
            tokens: 6,
            limit: 3
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SessionEvent::TurnStarted(_)))
        );
    }

    #[tokio::test]
    async fn runs_tools_until_the_model_answers() {
        let state = session_state("tool_loop").await;
        let provider = provider(vec![
            vec![Step::Call("ping"), Step::Done],
            vec![Step::Text("got ping"), Step::Done],
        ]);
        let (runner, input, mut output) = runner(&state, &provider, model(None));
        input.send("use the tool").unwrap();
        drop(input);

        runner.run().await.unwrap();

        let mut finished = None;
        while let Some(event) = output.next_event().await {
            if let SessionEvent::ToolFinished { result, .. } = event {
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
}
