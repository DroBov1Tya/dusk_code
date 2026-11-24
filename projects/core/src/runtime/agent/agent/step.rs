//! One step of a turn: a model call and the tools it asks for.

use super::calls::Called;
use super::wait::waits_out;
use super::{Agent, StepEnd, insert_project, interrupted, missing_tools, recorded_reasoning};
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Inbox, Loaded, repeats};
use crate::{
    message::sources::{SourceBlock, ThreadSources},
    providers::{
        ChatMessage, ChatRequest, ProviderErrorKind, ToolSpec, is_output_limit,
        is_unsupported_pictures, text_calls,
    },
    runtime::goal::command as goal,
    state::{
        CallPurpose, MessageRole, ModelCall, ProviderFailure, Turn, TurnAnswer, TurnFailure,
        TurnOutcome, estimate_tokens,
    },
};
use anyhow::{Context, Result};
use dusk_protocol::thread::RepeatRung;
use tracing::info;

/// An answer this short may stand beside a call the model wrote in its reasoning
/// ("Let me look."); a longer one is the answer, and its reasoning is left alone.
const SHORT_ANSWER: usize = 200;

impl Agent {
    /// Database writes happen outside `race`, so stopping never cuts one in half.
    /// `waited` says the turn's provider wait is stored, to end once the model answers.
    /// `note` goes to the model last, for this step only. With `reads_text`, the
    /// answer is cut once its text goes round (`limits.text_loops`).
    #[tracing::instrument(name = "step", skip_all)]
    pub(super) async fn step(
        &self,
        turn: &Turn,
        inbox: &mut Inbox,
        waited: bool,
        loaded: Option<Loaded>,
        note: Option<String>,
        reads_text: bool,
    ) -> Result<StepEnd> {
        // One snapshot per step: switches flipped meanwhile apply from the next step.
        let config = self.config_now().await;
        // A team's member steps only while it holds its task and the team has budget.
        crate::runtime::team::may_step(&self.state, &config, &self.thread_id, self.depth).await?;
        let stub_results = config.limits().stub_results;
        let mut conversation = Loaded::take(loaded, self, stub_results).await?;
        self.put_away_old_results(&mut conversation, &config).await;
        // Messages that joined between steps are answered by this turn, not a next one.
        let newest_user = conversation
            .messages
            .iter()
            .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Job))
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0);
        if newest_user > conversation.answered_through {
            self.state.extend_turn(&turn.turn_id, newest_user).await?;
        }
        let mut tools = self.tools.specs(&self.scope, &config);
        let screen = |spec: &ToolSpec| {
            self.tools.kind_of(&spec.name) == Some(crate::tools::ToolKind::Computer)
        };
        if tools.iter().any(screen) && !self.sees_pictures().await {
            tools.retain(|spec| !screen(spec));
        }
        let standing = self.active_goal().await?;
        let workspace = self.scope.workspace();
        let with_pictures = self.takes_pictures();
        let sources = ThreadSources {
            state: &self.state,
            workspace,
            thread_id: &self.thread_id,
        }
        .block(with_pictures)
        .await?;
        // The provider's last count already holds the project text.
        let project = match conversation.last_prompt {
            Some(_) => 0,
            None => estimate_tokens(&self.project),
        };
        let attached = project + sources.as_ref().map_or(0, SourceBlock::tokens);
        // Tool results grow the context within a turn, so every step checks it.
        if let Some(error) = self.context_too_large(&conversation, &tools, attached) {
            let provider = None;
            return Ok(StepEnd::TooLarge(TurnFailure { error, provider }));
        }
        let mut messages = conversation.chat_messages();
        if with_pictures {
            self.attach_tool_pictures(&conversation, &mut messages);
        }
        insert_project(&mut messages, &self.project);
        let first_time = sources.map(|block| {
            messages.push(block.message());
            block.first_time
        });
        // The goal is said last, so it stays in view however long the context grows.
        if let Some(goal) = standing {
            messages.push(ChatMessage::System(goal::reminder(&goal)?));
        }
        if let Some(note) = missing_tools(&messages, &tools) {
            messages.push(ChatMessage::System(note));
        }
        if let Some(note) = note {
            messages.push(ChatMessage::System(note));
        }
        let choice = self.model.current().context("provider is unavailable")?;
        let limits = config.limits();
        let chant = (reads_text && limits.text_loops).then_some(limits.text_loop_repeats);
        let set = self.effort(&config).await;
        let effort = self.pace.lock().effort(set);
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &tools,
            max_output_tokens: choice.model.max_output,
            effort,
            cache_key: Some(&self.thread_id),
        };
        let mut called = self
            .call_model(&choice, request, &config, chant, inbox)
            .await;
        let pictures_refused = matches!(
            &called,
            Ok(Called { generated: Ok(Err(error)), .. }) if is_unsupported_pictures(error)
        );
        if pictures_refused {
            self.refuse_pictures(&choice.model.id, &mut messages);
            let request = ChatRequest {
                model: &choice.model.id,
                messages: &messages,
                tools: &tools,
                max_output_tokens: choice.model.max_output,
                effort,
                cache_key: Some(&self.thread_id),
            };
            called = self
                .call_model(&choice, request, &config, chant, inbox)
                .await;
        }
        let Called {
            output,
            generated,
            attempts,
            chanted,
        } = match called {
            Ok(called) => called,
            Err(stop) => return Ok(StepEnd::Stopped(stop, TurnOutcome::Interrupted(None))),
        };
        // Only the last attempt is recorded: a failed one rarely gets usage reported.
        if let Some(usage) = output.usage {
            // The log says what each call cost and when the provider's cache let go, so
            // a drop can be told from the usage record afterwards.
            let before = conversation
                .last_prompt
                .as_ref()
                .map_or(0, |last| last.prompt_tokens);
            tracing::info!(
                prompt = usage.prompt_tokens,
                cached = usage.cached_prompt_tokens,
                written = usage.cache_write_tokens,
                completion = usage.completion_tokens,
                "model call"
            );
            if before > 4096 && usage.cached_prompt_tokens * 2 < before {
                tracing::warn!(
                    previous_prompt = before,
                    cached = usage.cached_prompt_tokens,
                    "prompt cache dropped: less than half of the last prompt was read from it"
                );
            }
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
                return Ok(StepEnd::Answered(TurnOutcome::failed(error)));
            }
            self.report_progress(&turn.turn_id).await;
        }
        // The answer went round: the step is taken again. A stream that ended before the
        // cut took effect went round all the same.
        if let (Some(_), Ok(_), Some(repeats)) = (chanted, &generated, chant) {
            info!("thread {}: the model's text went round", self.thread_id);
            // Told as a cycle: the rungs were fixed for all of the loop guards in one
            // protocol version, with none for text. The log line above tells them apart.
            self.emit(ThreadEvent::RepeatNotice {
                rung: RepeatRung::Cycle,
                count: repeats,
            });
            if waited {
                self.state.end_wait(&turn.turn_id).await?;
            }
            return Ok(StepEnd::Chanted(repeats::chant_note(repeats)));
        }
        let mut reasoning = recorded_reasoning(&config, &output);
        let mut stranded = None;
        let (content, generated) = match generated {
            Ok(Ok(calls)) if calls.is_empty() => {
                // A call in the reasoning counts only beside little or no answer.
                let short = output.text.trim().len() <= SHORT_ANSWER;
                let in_reasoning = || {
                    let (rest, calls) = text_calls::recover(&output.reasoning, &tools)?;
                    Some((output.text.clone(), calls, Some(rest)))
                };
                let found = text_calls::recover(&output.text, &tools)
                    .map(|(content, calls)| (content, calls, None))
                    .or_else(|| short.then(in_reasoning).flatten());
                match found {
                    Some((content, calls, rest)) => {
                        info!(
                            "thread {}: {} tool calls written as text",
                            self.thread_id,
                            calls.len()
                        );
                        // The recorded reasoning keeps the thought, not the markup.
                        if let Some(rest) = rest {
                            reasoning = reasoning.and(Some(rest).filter(|r| !r.trim().is_empty()));
                        }
                        (content, Ok(Ok(calls)))
                    }
                    None => {
                        stranded = text_calls::stranded(&output.text, &tools).or_else(|| {
                            short
                                .then(|| text_calls::stranded(&output.reasoning, &tools))
                                .flatten()
                        });
                        (output.text, Ok(Ok(calls)))
                    }
                }
            }
            generated => (output.text, generated),
        };
        let answer = TurnAnswer { reasoning, content };
        // A model that answered, or wrote anything, has read the sources. They are marked
        // now, not when the turn ends: the block is rebuilt every step, so a mark kept for
        // the turn's end sent them in full, uncached, with every step until then.
        let read = matches!(generated, Ok(Ok(_))) || !answer.content.trim().is_empty();
        if let Some(ids) = first_time.filter(|ids| read && !ids.is_empty())
            && let Err(error) = self.state.mark_sources_delivered(&ids).await
        {
            tracing::warn!(
                "thread {}: cannot mark sources delivered: {error:#}",
                self.thread_id
            );
        }
        let calls = match generated {
            Err(stop) => return Ok(StepEnd::Stopped(stop, interrupted(answer))),
            // The connection is closed already; what was written so far is kept.
            Ok(Err(error)) if is_output_limit(&error) => {
                tracing::warn!(
                    model = choice.model.id,
                    finish_reason = "length",
                    completion_tokens = output.usage.map(|usage| usage.completion_tokens),
                    "provider stopped at its output token limit"
                );
                self.emit(ThreadEvent::OutputLimitReached);
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
                let failed = crate::providers::provider_error(&error);
                let provider = failed.map(|failure| {
                    tracing::warn!(
                        kind = ?failure.kind,
                        status = failure.status,
                        cause = failure.cause.as_deref(),
                        attempts,
                        "model call failed: {}",
                        failure.message
                    );
                    ProviderFailure {
                        kind: failure.kind,
                        attempts,
                    }
                });
                let wait = failed
                    .filter(|failure| config.limits().wait_for_provider && waits_out(failure.kind))
                    .cloned();
                let too_long =
                    failed.is_some_and(|failure| failure.kind == ProviderErrorKind::TooLong);
                let failure = TurnFailure {
                    error: format!("{error:#}"),
                    provider,
                };
                return Ok(match wait {
                    Some(wait) => StepEnd::Wait(wait, failure),
                    None if too_long => StepEnd::TooLarge(failure),
                    None => StepEnd::Answered(TurnOutcome::Failed(failure)),
                });
            }
            Ok(Ok(calls)) if calls.is_empty() => {
                return Ok(match stranded {
                    Some(note) => {
                        info!(
                            "thread {}: a tool call written as text did not run",
                            self.thread_id
                        );
                        if waited {
                            self.state.end_wait(&turn.turn_id).await?;
                        }
                        StepEnd::Stranded(note, answer)
                    }
                    None => StepEnd::Answered(TurnOutcome::Completed(answer)),
                });
            }
            Ok(Ok(calls)) => calls,
        };
        // The model answered: the waits for it are over, and a later one counts anew.
        if waited {
            self.state.end_wait(&turn.turn_id).await?;
        }

        // On record before anything runs, so no tool takes effect without a trace.
        let recorded = self
            .state
            .record_tool_calls(&turn.turn_id, answer, calls.clone());
        if let Err(error) = recorded.await {
            let error = format!("cannot store the tool calls: {error:#}");
            return Ok(StepEnd::Answered(TurnOutcome::failed(error)));
        }
        Ok(self.run_calls(turn, &calls, &config, inbox).await)
    }
}
