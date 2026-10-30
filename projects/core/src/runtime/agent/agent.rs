//! The agent: runs a turn over a thread step by step. Each step streams the model's
//! response; when it calls tools, they run and the next step sends their results back,
//! until the model answers without calling a tool.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::warn;

use super::{Inbox, Stop, generation};
use crate::runtime::{
    chat::{Conversation, spec_tokens},
    system::{
        ThreadEvent,
        model_choice::{Choice, ModelChoice},
    },
};
use crate::{
    config::Config,
    live_config::LiveConfig,
    providers::{ChatRequest, ModelOutput, ToolCall, ToolSpec, is_output_limit, is_transient},
    state::{CallPurpose, MessageRole, ModelCall, State, Turn, TurnAnswer, TurnOutcome},
    tools::{Progress, ToolRegistry, ToolScope},
};

/// Bounds a model that keeps calling tools.
const MAX_STEPS: usize = 50;
/// A model call that fails before writing anything is tried this many times in all.
const MAX_ATTEMPTS: u32 = 3;
/// Doubles before each further attempt.
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);

pub struct Agent {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub tools: ToolRegistry,
    pub scope: ToolScope,
    pub thread_id: String,
    /// COMPACT.md: how to summarize the thread.
    pub compact_prompt: PathBuf,
    /// How many agents are above this thread; 0 for a chat.
    pub depth: usize,
    pub events: mpsc::UnboundedSender<ThreadEvent>,
}

/// How one step ended.
enum StepEnd {
    Answered(TurnOutcome),
    CalledTools,
    Stopped(Stop, TurnOutcome),
}

impl Agent {
    /// Marks turns left running by a stopped process as interrupted.
    pub async fn recover(&self) -> Result<()> {
        self.state.interrupt_running_turns(&self.thread_id).await
    }

    /// Starts a turn when something is queued and the context fits the input limit.
    pub async fn start_turn(&self) -> Result<Option<Turn>> {
        if self.model.current().is_none() {
            return Ok(None);
        }
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        let specs = self.tools.specs(&self.scope, &self.config.current());
        if !conversation.has_queued() || self.context_too_large(&conversation, &specs).is_some() {
            return Ok(None);
        }
        let turn = self.state.start_turn(&self.thread_id).await?;
        if let Some(turn) = &turn {
            self.emit(ThreadEvent::TurnStarted(turn.clone()));
        }
        Ok(turn)
    }

    /// Runs the turn to its end and records it. Returns why it stopped early, if it did.
    pub async fn run_turn(&self, turn: &Turn, inbox: &mut Inbox) -> Result<Option<Stop>> {
        let mut outcome =
            TurnOutcome::Failed(format!("the model made more than {MAX_STEPS} tool steps"));
        let mut stop = None;
        for _ in 0..MAX_STEPS {
            match self.step(turn, inbox).await? {
                // Tool results are what fills the context, so it is checked after each.
                StepEnd::CalledTools => match self.compact_if_needed(Some(turn), inbox).await {
                    None => {
                        self.take_agent_messages().await?;
                        continue;
                    }
                    Some(reason) => {
                        stop = Some(reason);
                        outcome = TurnOutcome::Interrupted(None);
                    }
                },
                StepEnd::Answered(answer) => outcome = answer,
                StepEnd::Stopped(reason, partial) => {
                    stop = Some(reason);
                    outcome = partial;
                }
            }
            break;
        }
        let turn = self.state.finish_turn(&turn.turn_id, outcome).await?;
        self.emit(ThreadEvent::TurnFinished(turn));
        self.report_tokens().await;
        Ok(stop)
    }

    /// Messages from other agents join the running turn, so the next step reads them.
    /// What clients sent waits for the next turn.
    async fn take_agent_messages(&self) -> Result<()> {
        for pending in self.state.pending_messages(&self.thread_id).await? {
            if pending.from_thread_id.is_none() {
                continue;
            }
            let message = self.state.admit_pending(&self.thread_id, &pending).await?;
            self.emit(ThreadEvent::Queued {
                message_id: message.message_id.clone(),
            });
            self.emit(ThreadEvent::FromAgent(message));
        }
        Ok(())
    }

    /// Counting is for the client to see; a failure here does not touch the thread.
    pub async fn report_tokens(&self) {
        let specs = self.tools.specs(&self.scope, &self.config.current());
        let counted = async {
            let mut tokens = self.state.thread_tokens(&self.thread_id).await?;
            let conversation = Conversation::load(&self.state, &self.thread_id).await?;
            tokens.current.tool_specs = spec_tokens(&specs);
            tokens.context = conversation.tokens(&specs);
            anyhow::Ok(tokens)
        };
        match counted.await {
            Ok(tokens) => self.emit(ThreadEvent::Tokens {
                tokens,
                limit: self.input_limit(),
            }),
            Err(error) => warn!("cannot count thread tokens: {error:#}"),
        }
    }

    pub fn emit(&self, event: ThreadEvent) {
        // A client that stopped listening does not stop the thread.
        let _ = self.events.send(event);
    }

    /// Database writes happen outside `race`, so stopping never cuts one in half.
    async fn step(&self, turn: &Turn, inbox: &mut Inbox) -> Result<StepEnd> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        // Messages that joined between steps are answered by this turn, not a next one.
        let newest_user = conversation
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0);
        if newest_user > conversation.answered_through {
            self.state.extend_turn(&turn.turn_id, newest_user).await?;
        }
        // One snapshot per step: switches flipped meanwhile apply from the next step.
        let config = self.config.current();
        let tools = self.tools.specs(&self.scope, &config);
        // Tool results grow the context within a turn, so every step checks it.
        if let Some(error) = self.context_too_large(&conversation, &tools) {
            return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
        }
        let messages = conversation.chat_messages();
        let choice = self.model.current().context("provider is unavailable")?;
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &tools,
            max_output_tokens: config.limits().max_output_tokens,
        };
        let (output, generated) = match self.call_model(&choice, request, inbox).await {
            Ok(called) => called,
            Err(stop) => return Ok(StepEnd::Stopped(stop, TurnOutcome::Interrupted(None))),
        };
        // Only the last attempt is recorded: a failed one rarely gets usage reported.
        if let Some(usage) = output.usage {
            let call = ModelCall {
                thread_id: self.thread_id.clone(),
                turn_id: Some(turn.turn_id.clone()),
                purpose: CallPurpose::Answer,
                compaction_id: conversation.compaction.as_ref().map(|c| c.compaction_id),
                model: choice.model.id.clone(),
                through_sequence: conversation.newest_sequence(),
            };
            if let Err(error) = self.state.record_usage(&call, usage).await {
                let error = format!("cannot store the model usage: {error:#}");
                return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
            }
        }
        let answer = TurnAnswer {
            reasoning: recorded_reasoning(&config, &output),
            content: output.text,
        };
        let calls = match generated {
            Err(stop) => return Ok(StepEnd::Stopped(stop, interrupted(answer))),
            // The connection is closed already; what was written so far is kept.
            Ok(Err(error)) if is_output_limit(&error) => {
                let limit = config.limits().max_output_tokens;
                self.emit(ThreadEvent::OutputLimitReached { limit });
                return Ok(StepEnd::Answered(interrupted(answer)));
            }
            // The connection broke after the model wrote something: what it wrote stays in
            // the thread, so the next turn can go on from it.
            Ok(Err(error)) if !answer.content.trim().is_empty() => {
                let error = format!("{error:#}");
                self.emit(ThreadEvent::AnswerCut { error });
                return Ok(StepEnd::Answered(interrupted(answer)));
            }
            Ok(Err(error)) => {
                if is_transient(&error) {
                    self.emit(ThreadEvent::ProviderUnavailable);
                }
                return Ok(StepEnd::Answered(TurnOutcome::Failed(format!("{error:#}"))));
            }
            Ok(Ok(calls)) if calls.is_empty() => {
                return Ok(StepEnd::Answered(TurnOutcome::Completed(answer)));
            }
            Ok(Ok(calls)) => calls,
        };

        // On record before anything runs, so no tool takes effect without a trace.
        let recorded = self
            .state
            .record_tool_calls(&turn.turn_id, answer, calls.clone());
        if let Err(error) = recorded.await {
            let error = format!("cannot store the tool calls: {error:#}");
            return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
        }
        Ok(match self.call_tools(turn, &calls, &config, inbox).await {
            Err(error) => StepEnd::Answered(TurnOutcome::Failed(format!("{error:#}"))),
            Ok(Some(stop)) => StepEnd::Stopped(stop, TurnOutcome::Interrupted(None)),
            Ok(None) => StepEnd::CalledTools,
        })
    }

    /// Streams the model's response, trying again when the call fails in a way that may
    /// pass and nothing reached the user yet. The outer `Err` means the user stopped it
    /// during a pause between attempts.
    pub async fn call_model(
        &self,
        choice: &Choice,
        request: ChatRequest<'_>,
        inbox: &mut Inbox,
    ) -> Result<(ModelOutput, Result<Result<Vec<ToolCall>>, Stop>), Stop> {
        let mut attempt = 1;
        loop {
            let mut output = ModelOutput::default();
            let events = self.events.clone();
            let streamed = generation::generate(&*choice.provider, request, &mut output, |event| {
                let _ = events.send(ThreadEvent::Model(event.clone()));
            });
            let generated = inbox.race(streamed).await;
            let error = match &generated {
                Ok(Err(error))
                    if attempt < MAX_ATTEMPTS && output.is_empty() && is_transient(error) =>
                {
                    format!("{error:#}")
                }
                _ => return Ok((output, generated)),
            };
            self.emit(ThreadEvent::Retrying { attempt, error });
            let pause = FIRST_RETRY_DELAY * 2u32.pow(attempt - 1);
            inbox.race(tokio::time::sleep(pause)).await?;
            attempt += 1;
        }
    }

    /// Calls run one by one, and each result is stored as soon as it is known. After a
    /// stop the rest are not run, but each still gets a result, because the model expects
    /// one per call. An error means a result could not be stored.
    async fn call_tools(
        &self,
        turn: &Turn,
        calls: &[ToolCall],
        config: &Config,
        inbox: &mut Inbox,
    ) -> Result<Option<Stop>> {
        let mut stop = None;
        for call in calls {
            let content = match stop {
                Some(_) => "not run: the user interrupted the turn".to_owned(),
                None => {
                    self.emit(ThreadEvent::ToolStarted(call.clone()));
                    let progress = Progress {
                        call_id: &call.id,
                        events: &self.events,
                    };
                    let called = self.tools.call(call, &self.scope, config, progress);
                    match inbox.race(called).await {
                        Ok(result) => {
                            let content = result.content.clone();
                            self.emit(ThreadEvent::ToolFinished {
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
            let stored = self
                .state
                .record_tool_result(&turn.turn_id, &call.id, content);
            stored.await.context("cannot store the tool result")?;
        }
        Ok(stop)
    }

    /// The largest request: the configured input limit, or the model window when smaller.
    pub fn input_limit(&self) -> u64 {
        let configured = self.config.current().limits().max_input_tokens;
        self.model
            .current()
            .and_then(|choice| choice.model.context_window)
            .map_or(configured, |window| window.min(configured))
    }

    /// Reports a context past the input limit to the client and returns why.
    fn context_too_large(&self, conversation: &Conversation, specs: &[ToolSpec]) -> Option<String> {
        let tokens = conversation.tokens(specs);
        let limit = Some(self.input_limit()).filter(|&limit| tokens > limit)?;
        self.emit(ThreadEvent::ContextTooLarge { tokens, limit });
        Some(format!(
            "the context is about {tokens} tokens, past the input limit of {limit}"
        ))
    }
}

/// What a stopped step keeps: the text written so far, if any.
fn interrupted(answer: TurnAnswer) -> TurnOutcome {
    let written = !answer.content.trim().is_empty();
    TurnOutcome::Interrupted(written.then_some(answer))
}

fn recorded_reasoning(config: &Config, output: &ModelOutput) -> Option<String> {
    let keep = config.thread().record.records_reasoning() && !output.reasoning.trim().is_empty();
    keep.then(|| output.reasoning.clone())
}
