//! The model call with its retries, the tool calls and the hooks around them.

use super::retry::{Bound, MAX_RETRY_DELAY, retry_delay};
use super::wait::waits_out;
use super::{Agent, Pace, StepEnd};
use crate::plugins::{HookEvent, hooks::Outcome};
use crate::runtime::agent::{Inbox, Stop, generation, repeats};
use crate::runtime::{ThreadEvent, model_choice::Choice};
use crate::{
    config::{Config, Effort, Limits},
    providers::{
        ChatRequest, ModelOutput, ProviderError, ToolCall, is_unsupported_effort, provider_error,
    },
    state::{GoalStatus, KIND_MCP_TOOL, KIND_SKILL, Turn, TurnOutcome},
    tools::{Progress, ToolKind, ToolResult},
};
use anyhow::{Context, Result, anyhow};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Instant};
use tracing::{info, warn};

/// What the calls after a stop by the user get.
const USER_STOPPED: &str = "not run: the user interrupted the turn";

/// What the calls after a stop by the failure ladder get.
const LADDER_STOPPED: &str = "not run: the turn stopped because the same error kept coming back";

/// Read-only calls of one step that run at once.
const TOGETHER: usize = 8;

/// What a call came to, before its result is stored.
struct Ran {
    content: String,
    outcome: &'static str,
    /// The tool's own result; `None` when the call did not run or was stopped.
    finished: Option<ToolResult>,
}

impl Ran {
    fn of(result: ToolResult) -> Self {
        Self {
            content: result.content.clone(),
            outcome: if result.is_error {
                "failed"
            } else {
                "succeeded"
            },
            finished: Some(result),
        }
    }

    fn cancelled(content: &str) -> Self {
        Self {
            content: content.to_owned(),
            outcome: "cancelled",
            finished: None,
        }
    }

    fn failed(content: String) -> Self {
        Self {
            content,
            outcome: "failed",
            finished: None,
        }
    }
}

/// How far a call of a group came.
#[derive(Default)]
enum Slot {
    /// Not started, or not run at all.
    #[default]
    Idle,
    /// Cut off by a stop while it ran.
    Started,
    Done(ToolResult),
}

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
    /// a pause between attempts. The last failure comes back as it was.
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
                    .filter(|failure| !self.stores_pause(failure, &limits))
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

    /// A turn that may wait for its provider stores a pause longer than the loop sleeps,
    /// so a restart does not cut it short: the call ends here and the turn waits.
    fn stores_pause(&self, failure: &ProviderError, limits: &Limits) -> bool {
        limits.wait_for_provider
            && waits_out(failure.kind)
            && failure
                .retry_after
                .is_some_and(|after| after > MAX_RETRY_DELAY)
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

    /// Runs the calls of a step and says how the step ended.
    pub(super) async fn run_calls(
        &self,
        turn: &Turn,
        calls: &[ToolCall],
        config: &Arc<Config>,
        inbox: &mut Inbox,
    ) -> StepEnd {
        match self.call_tools(turn, calls, config, inbox).await {
            Err(error) => StepEnd::Answered(TurnOutcome::failed(format!("{error:#}"))),
            Ok(Some(stop)) => StepEnd::Stopped(stop, TurnOutcome::Interrupted(None)),
            Ok(None) => StepEnd::CalledTools,
        }
    }

    /// Calls run in order, each result stored as soon as it is known, except that a run
    /// of read-only calls runs together and has its results stored in call order once
    /// all are in. After a stop, by the user or by the same error coming back too often,
    /// the rest are not run, but each still gets a result, because the model expects one
    /// per call. An error means a result could not be stored.
    pub(super) async fn call_tools(
        &self,
        turn: &Turn,
        calls: &[ToolCall],
        config: &Arc<Config>,
        inbox: &mut Inbox,
    ) -> Result<Option<Stop>> {
        let together = config.limits().parallel_reads;
        let mut stop = None;
        let mut surprised = false;
        let mut rest = calls;
        while !rest.is_empty() {
            let size = if together {
                self.reads_at_start(rest)
            } else {
                1
            };
            let (group, after) = rest.split_at(size);
            rest = after;
            let mut refused = Vec::new();
            for call in group {
                refused.push(self.refusal(turn, call, stop.is_some()).await?);
            }
            let running: Vec<usize> = (0..group.len())
                .filter(|&index| refused[index].is_none())
                .collect();
            let (mut slots, stopped) = self.run_group(group, &running, config, inbox).await;
            stop = stop.or(stopped);
            for (index, call) in group.iter().enumerate() {
                let ran = match refused[index].take() {
                    Some(ran) => ran,
                    None => match std::mem::take(&mut slots[index]) {
                        // A call that ran keeps its result unless an earlier one of its
                        // group stopped the turn on repeated errors: as when run one by
                        // one, it would not have run.
                        Slot::Done(result) if !self.repeats.lock().stopping(&turn.turn_id) => {
                            Ran::of(result)
                        }
                        Slot::Done(_) => Ran::cancelled(LADDER_STOPPED),
                        Slot::Started => Ran::cancelled("stopped: the user interrupted the turn"),
                        Slot::Idle => Ran::cancelled(USER_STOPPED),
                    },
                };
                surprised |= self.settle(turn, call, config, ran).await?;
            }
        }
        *self.pace.lock() = if surprised {
            Pace::Rethink
        } else {
            Pace::Follow
        };
        Ok(stop)
    }

    /// How many calls at the start of `calls` run together: the read-only ones, else one.
    fn reads_at_start(&self, calls: &[ToolCall]) -> usize {
        let reads = calls
            .iter()
            .take_while(|call| self.tools.read_only(&call.name, &call.arguments))
            .count();
        reads.max(1)
    }

    /// Why a call does not run, as the result it gets instead; `None` when it runs.
    /// Each call gets a result, so the model learns why the rest did not run.
    async fn refusal(&self, turn: &Turn, call: &ToolCall, stopped: bool) -> Result<Option<Ran>> {
        if stopped {
            return Ok(Some(Ran::cancelled(USER_STOPPED)));
        }
        if self.repeats.lock().stopping(&turn.turn_id) {
            return Ok(Some(Ran::cancelled(LADDER_STOPPED)));
        }
        let draft = self.depth == 0
            && self
                .state
                .goal(&self.thread_id)
                .await?
                .is_some_and(|g| !g.approved && g.status != GoalStatus::Done);
        let denied = match self.hooks.is_empty() {
            true => None,
            false => {
                let within = self.tools.calls_within(&call.name, &call.arguments);
                let base = json!({ "thread": self.thread_id, "depth": self.depth });
                let checked = self
                    .hooks
                    .before_tool(&call.name, &call.arguments, &within, &base);
                checked.await
            }
        };
        Ok(match denied {
            _ if draft && call.name != "goal" => Some(Ran::failed(
                "error: draft goal execution is blocked until the user approves its acceptance contract".to_owned(),
            )),
            Some(denied) => Some(Ran::failed(format!("error: {denied}"))),
            None => None,
        })
    }

    /// Runs `group[i]` for each `i` in `running`, at most `TOGETHER` at once, until all are
    /// done or the user stops them. Each slot says how far its call came, so a stop keeps
    /// the results of the calls that finished.
    async fn run_group(
        &self,
        group: &[ToolCall],
        running: &[usize],
        config: &Arc<Config>,
        inbox: &mut Inbox,
    ) -> (Vec<Slot>, Option<Stop>) {
        let mut slots: Vec<Slot> = group.iter().map(|_| Slot::Idle).collect();
        let started: Vec<AtomicBool> = group.iter().map(|_| AtomicBool::new(false)).collect();
        let stopped = {
            let slots = &mut slots;
            let started = &started;
            let work = async move {
                // Boxed, or the compiler cannot show the spawned turn is `Send`.
                let calls: Vec<BoxFuture<'_, (usize, ToolResult)>> = running
                    .iter()
                    .map(|&index| {
                        async move {
                            started[index].store(true, Ordering::Relaxed);
                            (index, self.run_call(&group[index], config).await)
                        }
                        .boxed()
                    })
                    .collect();
                let mut calls = stream::iter(calls).buffer_unordered(TOGETHER);
                while let Some((index, result)) = calls.next().await {
                    slots[index] = Slot::Done(result);
                }
            };
            inbox.race(work).await.err()
        };
        for (slot, started) in slots.iter_mut().zip(&started) {
            if matches!(slot, Slot::Idle) && started.load(Ordering::Relaxed) {
                *slot = Slot::Started;
            }
        }
        (slots, stopped)
    }

    async fn run_call(&self, call: &ToolCall, config: &Arc<Config>) -> ToolResult {
        self.emit(ThreadEvent::ToolStarted(call.clone()));
        let progress = Progress {
            call_id: call.id.clone(),
            events: self.events.clone(),
        };
        self.tools.call(call, &self.scope, config, progress).await
    }

    /// Everything after a call is known: hooks, counts, the note on a repeat, the stored
    /// result and the events. Says whether the call did not succeed.
    async fn settle(
        &self,
        turn: &Turn,
        call: &ToolCall,
        config: &Arc<Config>,
        ran: Ran,
    ) -> Result<bool> {
        let Ran {
            mut content,
            outcome,
            finished,
        } = ran;
        let shaped = finished
            .as_ref()
            .filter(|_| config.limits().shape_output)
            .and_then(|result| self.tools.shape(&call.name, &call.arguments, result));
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
        let notice = finished.as_ref().and_then(|result| {
            let limits = config.limits();
            let mut repeats = self.repeats.lock();
            repeats.note(&turn.turn_id, call, result, &content, &limits)
        });
        if let Some(notice) = &notice {
            content.push_str(&format!("\n\n{}", notice.note));
        }
        // A pass that ended a loop is written down before the result is stored, so the
        // lesson note is decided once, with the result, as the other notes are.
        let limits = config.limits();
        let solved = finished
            .as_ref()
            .filter(|_| limits.episodes)
            .and_then(|result| self.repeats.lock().solved(&turn.turn_id, call, result));
        if let Some(solved) = solved
            && self.record_fixed(&turn.turn_id, call, &solved).await
            && limits.episode_lesson
        {
            content.push_str(&format!("\n\n{}", repeats::lesson_note(solved.count)));
            self.repeats.lock().ask_lesson(&turn.turn_id);
        }
        // What hooks and the repeat note added goes to the model shaped or not.
        let shaped = shaped.zip(finished.as_ref()).and_then(|(shaped, result)| {
            let notes = content.strip_prefix(result.content.as_str())?;
            Some((shaped, notes.to_owned()))
        });
        let (artifacts, exit_code) = finished
            .as_ref()
            .map_or((Vec::new(), None), |r| (r.artifacts.clone(), r.exit_code));
        let stored = self.state.record_tool_result_with_outcome(
            &turn.turn_id,
            &call.id,
            content,
            artifacts,
            outcome,
            exit_code,
        );
        let stored = stored.await.context("cannot store the tool result")?;
        if let Some((shaped, notes)) = shaped {
            self.send_shaped(&stored, shaped, &notes).await;
        }
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
        // A re-plan needs no pace of its own: its call failed, so the next step
        // already runs at the effort set (`Pace::Rethink`).
        if let Some(notice) = notice {
            self.emit(ThreadEvent::RepeatNotice {
                rung: notice.rung,
                count: notice.count,
            });
        }
        Ok(outcome != "succeeded")
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
