//! The model call with its retries, the tool calls and the hooks around them.

use super::retry::{Bound, retry_delay};
use super::{Agent, Pace};
use crate::plugins::{HookEvent, hooks::Outcome};
use crate::runtime::agent::{Inbox, Stop, generation};
use crate::runtime::{ThreadEvent, model_choice::Choice};
use crate::{
    config::{Config, Effort},
    providers::{ChatRequest, ModelOutput, ToolCall, is_unsupported_effort, provider_error},
    state::{GoalStatus, KIND_MCP_TOOL, KIND_SKILL, Turn},
    tools::{Progress, ToolKind},
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};
use tracing::{info, warn};

/// What a model call came to after its attempts.
pub struct Called {
    pub output: ModelOutput,
    /// The outer `Err` means the user stopped it while it streamed.
    pub generated: Result<Result<Vec<ToolCall>>, Stop>,
    /// How many times the request was sent, the first included.
    pub attempts: u32,
}

impl Agent {
    /// Streams the model's response, trying again when the call fails in a way that may
    /// pass and nothing reached the user yet. The `Err` means the user stopped it during
    /// a pause between attempts.
    pub async fn call_model(
        &self,
        choice: &Choice,
        request: ChatRequest<'_>,
        requested_config: &Config,
        inbox: &mut Inbox,
    ) -> Result<Called, Stop> {
        let limits = self.config.current().limits();
        let idle = std::time::Duration::from_secs(limits.model_timeout_seconds.into());
        let started = tokio::time::Instant::now();
        let mut attempt = 1;
        loop {
            let mut output = ModelOutput::default();
            let events = self.events.clone();
            let mut meter = crate::runtime::agent::rate::RateMeter::default();
            let streamed =
                generation::generate(&*choice.provider, request, &mut output, idle, |event| {
                    if let Some(tokens_per_sec) = meter.observe(event, Instant::now()) {
                        self.emit(ThreadEvent::OutputRate {
                            tokens_per_sec,
                            window_ms: crate::runtime::agent::rate::WINDOW.as_millis() as u64,
                        });
                    }
                    let _ = events.send(ThreadEvent::Model(event.clone()));
                });
            let mut generated = inbox.race(streamed).await;
            if let Some(tokens_per_sec) = meter.current(Instant::now()) {
                self.emit(ThreadEvent::OutputRate {
                    tokens_per_sec,
                    window_ms: crate::runtime::agent::rate::WINDOW.as_millis() as u64,
                });
            }
            self.note_health(&choice.provider_name, &output, &generated);
            if request.effort == Some(Effort::Off)
                && let Ok(Err(error)) = &mut generated
                && is_unsupported_effort(error)
            {
                let mut restored = false;
                let saved = self.config.update(|config| {
                    restored = config.restore_unsupported_effort(requested_config);
                });
                let detail = match saved {
                    Ok(config) if restored => format!(
                        "previous reasoning effort restored to {}",
                        config.effort().map_or("default", Effort::name)
                    ),
                    Ok(_) => "reasoning effort changed meanwhile; current setting kept".to_owned(),
                    Err(save_error) => {
                        format!("cannot restore previous reasoning effort: {save_error:#}")
                    }
                };
                let rejected = std::mem::replace(error, anyhow!("reasoning effort rejected"));
                *error = rejected.context(detail);
            }
            let retry = match &generated {
                Ok(Err(error)) if !output.answered() => provider_error(error)
                    .filter(|failure| failure.kind.is_retryable())
                    .and_then(|failure| {
                        let pause = failure.retry_after.unwrap_or_else(|| retry_delay(attempt));
                        let bound = Bound::of(failure.kind, &limits);
                        bound
                            .allows(attempt, started.elapsed(), pause)
                            .then(|| (format!("{error:#}"), failure, pause, bound))
                    }),
                _ => None,
            };
            let Some((error, failure, pause, bound)) = retry else {
                return Ok(Called {
                    output,
                    generated,
                    attempts: attempt,
                });
            };
            let kind = failure.kind;
            warn!(
                kind = ?kind,
                status = failure.status,
                cause = failure.cause.as_deref(),
                "thread {} model call failed, retry {attempt}: {error}",
                self.thread_id
            );
            self.emit(ThreadEvent::Retrying {
                attempt,
                error,
                kind,
                wait_ms: u64::try_from(pause.as_millis()).unwrap_or(u64::MAX),
                max_attempts: bound.max_attempts(),
            });
            inbox.race(tokio::time::sleep(pause)).await?;
            attempt += 1;
        }
    }

    /// What a call says of its provider's health: a failure that is the provider's, or
    /// an answer. A stop says nothing.
    fn note_health(
        &self,
        provider: &str,
        output: &ModelOutput,
        generated: &Result<Result<Vec<ToolCall>>, Stop>,
    ) {
        let now = dusk_protocol::system::now_ms();
        let failure = match generated {
            Ok(Err(error)) => provider_error(error).map(|failure| failure.kind),
            Ok(Ok(_)) | Err(_) => None,
        };
        // A refused request was still answered; so was a call that wrote something.
        let answered = matches!(generated, Ok(Ok(_))) || failure.is_some() || output.answered();
        let fault = failure.filter(|kind| kind.is_providers_fault());
        let changed = match fault {
            Some(kind) => self.health.failed(provider, kind, now),
            None if answered => self.health.answered(provider, now),
            None => false,
        };
        match fault {
            Some(kind) if changed => info!("provider {provider} fails: {kind:?}"),
            None if changed => info!("provider {provider} answers"),
            _ => {}
        }
    }

    /// Calls run one by one, and each result is stored as soon as it is known. After a
    /// stop the rest are not run, but each still gets a result, because the model expects
    /// one per call. An error means a result could not be stored.
    pub(super) async fn call_tools(
        &self,
        turn: &Turn,
        calls: &[ToolCall],
        config: &Arc<Config>,
        inbox: &mut Inbox,
    ) -> Result<Option<Stop>> {
        let mut stop = None;
        let mut surprised = false;
        for call in calls {
            let mut artifacts = Vec::new();
            let mut finished = None;
            let mut outcome = "cancelled";
            let mut exit_code = None;
            let draft = self.depth == 0
                && self
                    .state
                    .goal(&self.thread_id)
                    .await?
                    .is_some_and(|g| !g.approved && g.status != GoalStatus::Done);
            let denied = match stop {
                None if !self.hooks.is_empty() => {
                    let within = self.tools.calls_within(&call.name, &call.arguments);
                    let base = json!({ "thread": self.thread_id, "depth": self.depth });
                    let checked =
                        self.hooks
                            .before_tool(&call.name, &call.arguments, &within, &base);
                    checked.await
                }
                _ => None,
            };
            let mut content = match stop {
                Some(_) => "not run: the user interrupted the turn".to_owned(),
                None if draft && call.name != "goal" => {
                    outcome = "failed";
                    "error: draft goal execution is blocked until the user approves its acceptance contract".to_owned()
                }
                None if let Some(denied) = &denied => {
                    outcome = "failed";
                    format!("error: {denied}")
                }
                None => {
                    self.emit(ThreadEvent::ToolStarted(call.clone()));
                    let progress = Progress {
                        call_id: call.id.clone(),
                        events: self.events.clone(),
                    };
                    let called = self.tools.call(call, &self.scope, config, progress);
                    match inbox.race(called).await {
                        Ok(result) => {
                            let content = result.content.clone();
                            outcome = if result.is_error {
                                "failed"
                            } else {
                                "succeeded"
                            };
                            exit_code = result.exit_code;
                            artifacts = result.artifacts.clone();
                            finished = Some(result);
                            content
                        }
                        Err(reason) => {
                            stop = Some(reason);
                            "stopped: the user interrupted the turn".to_owned()
                        }
                    }
                }
            };
            if finished.is_some() && !self.hooks.is_empty() {
                let details =
                    json!({ "tool": call.name, "arguments": call.arguments, "result": content });
                let notes = self.run_hooks(HookEvent::AfterTool, Some(&call.name), details);
                for note in notes.await.context {
                    content.push_str(&format!("\n\n{note}"));
                }
            }
            // Durable usage counts: skills loaded and MCP tools called, when the call
            // ran and did not fail.
            if finished.is_some() && outcome != "failed" {
                if let Some(skill) = self.tools.loaded_skill(&call.name, &call.arguments) {
                    if let Err(error) = self
                        .state
                        .record_item_use(Some(&self.thread_id), KIND_SKILL, &skill)
                        .await
                    {
                        warn!("cannot count the skill use: {error:#}");
                    }
                } else if self.tools.kind_of(&call.name) == Some(ToolKind::Mcp)
                    && let Err(error) = self
                        .state
                        .record_item_use(Some(&self.thread_id), KIND_MCP_TOOL, call.name.as_str())
                        .await
                {
                    warn!("cannot count the MCP tool use: {error:#}");
                }
            }
            if finished.is_some()
                && let Some(note) =
                    self.repeats
                        .lock()
                        .note(&turn.turn_id, &call.name, &call.arguments, &content)
            {
                content.push_str(&format!("\n\n{note}"));
            }
            surprised |= outcome != "succeeded";
            let stored = self.state.record_tool_result_with_outcome(
                &turn.turn_id,
                &call.id,
                content,
                artifacts,
                outcome,
                exit_code,
            );
            stored.await.context("cannot store the tool result")?;
            if call.name == "ask_user"
                && let Some(result) = &finished
                && !result.is_error
                && let Some(question) = call.arguments["question"].as_str()
            {
                self.asking.lock().push(question.to_owned());
            }
            // A visible result must already be available when the thread is reopened.
            if let Some(result) = finished {
                self.emit(ThreadEvent::ToolFinished {
                    call_id: call.id.clone(),
                    result,
                });
            }
        }
        *self.pace.lock() = if surprised {
            Pace::Rethink
        } else {
            Pace::Follow
        };
        Ok(stop)
    }

    /// Runs the plugins' hooks of `event` with what every hook is told about the thread.
    pub async fn run_hooks(
        &self,
        event: HookEvent,
        tool: Option<&str>,
        mut details: Value,
    ) -> Outcome {
        details["thread"] = json!(self.thread_id);
        details["depth"] = json!(self.depth);
        self.hooks.run(event, tool, details).await
    }

    /// Runs the hooks of `event` and keeps the notes they give for the model to read.
    pub async fn hook_notes(&self, event: HookEvent, details: Value) -> Result<()> {
        if self.hooks.is_empty() {
            return Ok(());
        }
        for note in self.run_hooks(event, None, details).await.context {
            self.state
                .add_system_message(&self.thread_id, &note)
                .await?;
        }
        Ok(())
    }
}
