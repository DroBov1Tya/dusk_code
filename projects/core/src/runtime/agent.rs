//! The agent: runs a turn over a thread step by step. Each step streams the model's
//! response; when it calls tools, they run and the next step sends their results back,
//! until the model answers without calling a tool.

use anyhow::Result;
use tokio::sync::mpsc;

use super::{
    context, generation,
    inbox::{Inbox, Stop},
    session_runner::SessionEvent,
};
use crate::{
    config::Config,
    live_config::LiveConfig,
    providers::{ChatRequest, Model, ModelOutput, ModelProvider, ToolCall},
    receivers::{AgentOutcome, AgentReceiver, AgentStep},
    state::{Message, StepResult, Turn},
    tools::{ToolRegistry, ToolScope},
};

/// Bounds a model that keeps calling tools.
const MAX_STEPS: usize = 50;

pub(super) struct Agent<'a, P> {
    pub(super) receiver: AgentReceiver<'a>,
    pub(super) provider: &'a P,
    pub(super) model: Model,
    pub(super) config: &'a LiveConfig,
    pub(super) tools: ToolRegistry,
    pub(super) scope: ToolScope,
    pub(super) thread_id: String,
    pub(super) events: mpsc::UnboundedSender<SessionEvent>,
}

/// How one step ended.
enum StepEnd {
    Answered(AgentOutcome),
    CalledTools,
    Stopped(Stop, AgentOutcome),
}

impl<'a, P: ModelProvider> Agent<'a, P> {
    pub(super) async fn recover(&self) -> Result<()> {
        self.receiver.recover(&self.thread_id).await
    }

    /// Starts a turn when something is queued and the context fits the model window.
    pub(super) async fn start_turn(&self) -> Result<Option<Turn>> {
        let conversation = self.receiver.conversation(&self.thread_id).await?;
        if !conversation.has_queued() {
            return Ok(None);
        }
        if self.context_too_large(&conversation.messages).is_some() {
            return Ok(None);
        }
        let turn = self.receiver.start_turn(&self.thread_id).await?;
        if let Some(turn) = &turn {
            self.emit(SessionEvent::TurnStarted(turn.clone()));
        }
        Ok(turn)
    }

    /// Runs the turn to its end and records it. Returns why it stopped early, if it did.
    pub(super) async fn run_turn(&self, turn: &Turn, inbox: &mut Inbox) -> Result<Option<Stop>> {
        let mut outcome = AgentOutcome::Failed {
            error_message: format!("the model made more than {MAX_STEPS} tool steps"),
        };
        let mut stop = None;
        for _ in 0..MAX_STEPS {
            match self.step(turn, inbox).await? {
                StepEnd::CalledTools => continue,
                StepEnd::Answered(answer) => outcome = answer,
                StepEnd::Stopped(reason, partial) => {
                    stop = Some(reason);
                    outcome = partial;
                }
            }
            break;
        }
        let turn = self.receiver.finish_turn(turn, outcome).await?;
        self.emit(SessionEvent::TurnFinished(turn));
        Ok(stop)
    }

    pub(super) fn emit(&self, event: SessionEvent) {
        // A client that stopped listening does not stop the session.
        let _ = self.events.send(event);
    }

    /// Database writes happen outside `race`, so stopping never cuts one in half.
    async fn step(&self, turn: &Turn, inbox: &mut Inbox) -> Result<StepEnd> {
        let conversation = self.receiver.conversation(&self.thread_id).await?;
        // Tool results grow the context within a turn, so every step checks it.
        if let Some(error_message) = self.context_too_large(&conversation.messages) {
            return Ok(StepEnd::Answered(AgentOutcome::Failed { error_message }));
        }
        // One snapshot per step: switches flipped meanwhile apply from the next step.
        let config = self.config.current();
        let messages = context::chat_messages(&conversation.messages);
        let tools = self.tools.specs(&self.scope, &config);
        let request = ChatRequest {
            model: &self.model.id,
            messages: &messages,
            tools: &tools,
        };
        let mut output = ModelOutput::default();
        let events = self.events.clone();
        let generated = inbox
            .race(generation::generate(
                self.provider,
                request,
                &mut output,
                |event| {
                    let _ = events.send(SessionEvent::Model(event.clone()));
                },
            ))
            .await;
        let reasoning = recorded_reasoning(&config, &output);
        let calls = match generated {
            Err(stop) => {
                let partial = AgentOutcome::Interrupted {
                    content: output.text,
                    reasoning,
                };
                return Ok(StepEnd::Stopped(stop, partial));
            }
            Ok(Err(error)) => {
                return Ok(StepEnd::Answered(AgentOutcome::Failed {
                    error_message: format!("{error:#}"),
                }));
            }
            Ok(Ok(calls)) if calls.is_empty() => {
                return Ok(StepEnd::Answered(AgentOutcome::Completed {
                    content: output.text,
                    reasoning,
                }));
            }
            Ok(Ok(calls)) => calls,
        };

        let (results, stop) = self.call_tools(&calls, &config, inbox).await;
        let step = AgentStep {
            content: output.text,
            reasoning,
            tool_calls: calls,
            results,
        };
        // The tools have run; failing the turn keeps the session usable.
        if let Err(error) = self.receiver.record_step(turn, step).await {
            return Ok(StepEnd::Answered(AgentOutcome::Failed {
                error_message: format!("cannot store the tool step: {error:#}"),
            }));
        }
        Ok(match stop {
            Some(stop) => StepEnd::Stopped(
                stop,
                AgentOutcome::Interrupted {
                    content: String::new(),
                    reasoning: None,
                },
            ),
            None => StepEnd::CalledTools,
        })
    }

    /// Calls run one by one. After a stop the rest are not run, but each still gets a
    /// result, because the model expects one per call.
    async fn call_tools(
        &self,
        calls: &[ToolCall],
        config: &Config,
        inbox: &mut Inbox,
    ) -> (Vec<StepResult>, Option<Stop>) {
        let mut results = Vec::with_capacity(calls.len());
        let mut stop = None;
        for call in calls {
            let content = match stop {
                Some(_) => "not run: the user interrupted the turn".to_owned(),
                None => {
                    self.emit(SessionEvent::ToolStarted(call.clone()));
                    match inbox.race(self.tools.call(call, &self.scope, config)).await {
                        Ok(result) => {
                            let content = result.content.clone();
                            self.emit(SessionEvent::ToolFinished {
                                call_id: call.id.clone(),
                                result,
                            });
                            content
                        }
                        Err(reason) => {
                            stop = Some(reason);
                            "stopped: the user interrupted the turn".to_owned()
                        }
                    }
                }
            };
            results.push(StepResult {
                call_id: call.id.clone(),
                content,
            });
        }
        (results, stop)
    }

    /// Reports a context past the model window to the client and returns why.
    fn context_too_large(&self, messages: &[Message]) -> Option<String> {
        let tokens = context::context_tokens(messages);
        let limit = self.model.context_window.filter(|&limit| tokens > limit)?;
        self.emit(SessionEvent::ContextTooLarge { tokens, limit });
        Some(format!(
            "the context is about {tokens} tokens, past the model window of {limit}"
        ))
    }
}

fn recorded_reasoning(config: &Config, output: &ModelOutput) -> Option<String> {
    let keep = config.session().record.records_reasoning() && !output.reasoning.trim().is_empty();
    keep.then(|| output.reasoning.clone())
}
