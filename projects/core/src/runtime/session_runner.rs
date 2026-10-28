//! A live chat session. Messages can be sent at any time; the agent runs one turn at a
//! time over everything queued. `send_now` stops the running turn, keeping what it wrote,
//! so the next turn starts at once with the new message.
//!
//! Messages sent during a turn are held in memory and stored right after the turn's
//! answer, so the thread always reads in the order things happened.

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{context, generation};
use crate::{
    config::RecordMode,
    providers::{ChatRequest, Model, ModelEvent, ModelOutput, ModelProvider, ToolCall},
    receivers::{AgentOutcome, AgentReceiver, UserMessage, UserReceiver},
    state::{State, Turn},
};

pub struct SessionRunner<'a, P> {
    users: UserReceiver<'a>,
    agent: AgentReceiver<'a>,
    provider: &'a P,
    model: Model,
    record: RecordMode,
    thread_id: String,
    cwd: String,
    commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<SessionEvent>,
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
    TurnFinished(Turn),
    /// The queued turn was not started; the client should switch model or compact.
    ContextTooLarge {
        tokens: u64,
        limit: u64,
    },
    /// Nothing is running and nothing will start until the next message.
    Idle,
}

enum Command {
    Send(String),
    SendNow(String),
    Cancel,
}

/// How a running turn ended.
enum Stop {
    Finished(Result<Vec<ToolCall>>),
    Cancelled,
    Preempted,
}

impl<'a, P: ModelProvider> SessionRunner<'a, P> {
    pub fn new(
        state: &'a State,
        provider: &'a P,
        model: Model,
        record: RecordMode,
        agent_id: &str,
        thread_id: &str,
        cwd: &str,
    ) -> Result<(Self, SessionInput, SessionOutput)> {
        let (command_sender, commands) = mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::unbounded_channel();
        let runner = Self {
            users: UserReceiver::new(state),
            agent: AgentReceiver::new(state, agent_id)?,
            provider,
            model,
            record,
            thread_id: thread_id.to_owned(),
            cwd: cwd.to_owned(),
            commands,
            events,
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
        self.agent.recover(&self.thread_id).await?;
        let mut input_open = true;
        loop {
            input_open = self.run_queued(input_open).await?;
            self.emit(SessionEvent::Idle);
            if !input_open {
                return Ok(());
            }
            let Some(command) = self.commands.recv().await else {
                return Ok(());
            };
            self.accept_idle(command).await;
            // Everything already sent goes into the same turn.
            while let Ok(command) = self.commands.try_recv() {
                self.accept_idle(command).await;
            }
        }
    }

    /// Runs turns until nothing is queued, the user cancels or the context does not fit.
    /// Returns whether the input is still open.
    async fn run_queued(&mut self, mut input_open: bool) -> Result<bool> {
        loop {
            let conversation = self.agent.conversation(&self.thread_id).await?;
            if !conversation.has_queued() {
                return Ok(input_open);
            }
            let tokens = context::context_tokens(&conversation.messages);
            if let Some(limit) = self.model.context_window.filter(|&limit| tokens > limit) {
                self.emit(SessionEvent::ContextTooLarge { tokens, limit });
                return Ok(input_open);
            }
            let Some(turn) = self.agent.start_turn(&self.thread_id).await? else {
                return Ok(input_open);
            };
            self.emit(SessionEvent::TurnStarted(turn.clone()));

            let messages = context::chat_messages(&conversation.messages);
            let request = ChatRequest {
                model: &self.model.id,
                messages: &messages,
                tools: &[],
            };
            let mut output = ModelOutput::default();
            let mut sent_during_turn = Vec::new();
            let stop = {
                let provider = self.provider;
                let events = self.events.clone();
                let generation = generation::generate(provider, request, &mut output, |event| {
                    let _ = events.send(SessionEvent::Model(event.clone()));
                });
                tokio::pin!(generation);
                loop {
                    tokio::select! {
                        result = &mut generation => break Stop::Finished(result),
                        command = self.commands.recv(), if input_open => match command {
                            Some(Command::Send(text)) => sent_during_turn.push(text),
                            Some(Command::SendNow(text)) => {
                                sent_during_turn.push(text);
                                break Stop::Preempted;
                            }
                            Some(Command::Cancel) => break Stop::Cancelled,
                            None => input_open = false,
                        },
                    }
                }
            };

            let cancelled = matches!(stop, Stop::Cancelled);
            let turn = self
                .agent
                .finish_turn(&turn, self.outcome(stop, output))
                .await?;
            self.emit(SessionEvent::TurnFinished(turn));
            for text in sent_during_turn {
                self.accept(text).await;
            }
            if cancelled {
                return Ok(input_open);
            }
        }
    }

    async fn accept_idle(&self, command: Command) {
        match command {
            Command::Send(text) | Command::SendNow(text) => self.accept(text).await,
            Command::Cancel => {}
        }
    }

    async fn accept(&self, text: String) {
        let message = UserMessage {
            request_uid: Uuid::now_v7().to_string(),
            thread_id: self.thread_id.clone(),
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
        self.emit(event);
    }

    fn outcome(&self, stop: Stop, output: ModelOutput) -> AgentOutcome {
        let reasoning = Some(output.reasoning)
            .filter(|reasoning| self.record.records_reasoning() && !reasoning.trim().is_empty());
        match stop {
            Stop::Finished(Ok(calls)) if !calls.is_empty() => AgentOutcome::Failed {
                error_message: "the model called a tool, but no tools are available yet".into(),
            },
            Stop::Finished(Ok(_)) => AgentOutcome::Completed {
                content: output.text,
                reasoning,
            },
            Stop::Finished(Err(error)) => AgentOutcome::Failed {
                error_message: format!("{error:#}"),
            },
            Stop::Cancelled | Stop::Preempted => AgentOutcome::Interrupted {
                content: output.text,
                reasoning,
            },
        }
    }

    /// A client that stopped listening does not stop the session.
    fn emit(&self, event: SessionEvent) {
        let _ = self.events.send(event);
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

    use super::*;
    use crate::{
        providers::{ChatStream, FinishReason},
        state::{ApprovalMode, MessageRole, ThreadInput, TurnStatus},
        testing::temp_state,
    };

    /// Each model call takes the next script; `Hang` never finishes, like a slow model.
    struct FakeProvider {
        scripts: Mutex<VecDeque<Vec<Step>>>,
    }

    #[derive(Clone, Copy)]
    enum Step {
        Text(&'static str),
        Hang,
        Done,
    }

    struct FakeStream(VecDeque<Step>);

    impl ModelProvider for FakeProvider {
        type Stream = FakeStream;

        async fn list_models(&self) -> Result<Vec<Model>> {
            Ok(Vec::new())
        }

        async fn stream_chat(&self, _request: ChatRequest<'_>) -> Result<FakeStream> {
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Ok(FakeStream(script.into()))
        }
    }

    impl ChatStream for FakeStream {
        async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
            match self.0.pop_front() {
                Some(Step::Text(text)) => Ok(Some(ModelEvent::Text(text.into()))),
                Some(Step::Hang) => std::future::pending().await,
                Some(Step::Done) => Ok(Some(ModelEvent::Finished(FinishReason::Stop))),
                None => Ok(None),
            }
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
        SessionRunner::new(
            state,
            provider,
            model,
            RecordMode::Full,
            "agent",
            "thread",
            "/tmp",
        )
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
}
