//! One step of a turn: a model call and the tools it asks for.

use super::{Agent, StepEnd, insert_project, interrupted, missing_tools, recorded_reasoning};
use crate::runtime::agent::Inbox;
use crate::runtime::{ThreadEvent, thread::Conversation};
use crate::{
    message::sources::{SourceBlock, ThreadSources},
    providers::{
        ChatMessage, ChatRequest, ToolSpec, is_output_limit, is_transient, is_unsupported_pictures,
        text_calls,
    },
    runtime::goal::action as goal,
    state::{
        CallPurpose, GoalStatus, MessageRole, ModelCall, Turn, TurnAnswer, TurnOutcome,
        estimate_tokens,
    },
};
use anyhow::{Context, Result};
use tracing::info;

impl Agent {
    /// Database writes happen outside `race`, so stopping never cuts one in half.
    #[tracing::instrument(name = "step", skip_all)]
    pub(super) async fn step(&self, turn: &Turn, inbox: &mut Inbox) -> Result<StepEnd> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
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
        // One snapshot per step: switches flipped meanwhile apply from the next step.
        let config = self.config.current();
        let mut tools = self.tools.specs(&self.scope, &config);
        let screen = |spec: &ToolSpec| {
            self.tools.kind_of(&spec.name) == Some(crate::tools::ToolKind::Computer)
        };
        if tools.iter().any(screen) && !self.sees_pictures().await {
            tools.retain(|spec| !screen(spec));
        }
        let standing = if self.depth == 0 {
            self.state.goal(&self.thread_id).await?
        } else {
            None
        };
        if standing
            .as_ref()
            .is_some_and(|g| !g.approved && g.status != GoalStatus::Done)
        {
            tools.retain(|tool| tool.name == "goal");
        }
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
            return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
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
        if let Some(goal) = standing.filter(|g| {
            g.status == GoalStatus::Active || (!g.approved && g.status != GoalStatus::Done)
        }) {
            let rounds = config.limits().max_goal_rounds;
            messages.push(ChatMessage::System(goal::reminder(&goal, rounds)));
        }
        if let Some(note) = missing_tools(&messages, &tools) {
            messages.push(ChatMessage::System(note));
        }
        let choice = self.model.current().context("provider is unavailable")?;
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
        let mut called = self.call_model(&choice, request, &config, inbox).await;
        if matches!(&called, Ok((_, Ok(Err(error)))) if is_unsupported_pictures(error)) {
            self.refuse_pictures(&choice.model.id, &mut messages);
            let request = ChatRequest {
                model: &choice.model.id,
                messages: &messages,
                tools: &tools,
                max_output_tokens: choice.model.max_output,
                effort,
                cache_key: Some(&self.thread_id),
            };
            called = self.call_model(&choice, request, &config, inbox).await;
        }
        let (output, generated) = match called {
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
                return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
            }
        }
        let reasoning = recorded_reasoning(&config, &output);
        let (content, generated) = match generated {
            Ok(Ok(calls)) if calls.is_empty() => match text_calls::recover(&output.text, &tools) {
                Some((content, calls)) => {
                    info!(
                        "thread {}: {} tool calls written as text",
                        self.thread_id,
                        calls.len()
                    );
                    (content, Ok(Ok(calls)))
                }
                None => (output.text, Ok(Ok(calls))),
            },
            generated => (output.text, generated),
        };
        let answer = TurnAnswer { reasoning, content };
        let calls = match generated {
            Err(stop) => return Ok(StepEnd::Stopped(stop, interrupted(answer))),
            // The connection is closed already; what was written so far is kept.
            Ok(Err(error)) if is_output_limit(&error) => {
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
                if is_transient(&error) {
                    self.emit(ThreadEvent::ProviderUnavailable);
                }
                return Ok(StepEnd::Answered(TurnOutcome::Failed(format!("{error:#}"))));
            }
            Ok(Ok(calls)) if calls.is_empty() => {
                // A turn cut short carries its new sources again next time.
                let first_time = first_time.unwrap_or_default();
                self.state.mark_sources_delivered(&first_time).await?;
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
}
