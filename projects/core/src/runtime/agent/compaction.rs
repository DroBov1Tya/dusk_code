//! Compaction: the model summarizes the context so far, and the summary takes the place
//! of the older messages. It runs on `/compact`, and on its own before the context
//! outgrows the model window.

use anyhow::{Context, Result, bail};
use tracing::warn;

use super::{Agent, Inbox, Stop};
use crate::runtime::{chat::Conversation, system::ThreadEvent};
use crate::{
    init::COMPACT_PROMPT_TEMPLATE,
    providers::{ChatMessage, ChatRequest},
    state::{CallPurpose, Compaction, ModelCall, Turn},
    thread_start::read_prompt,
};

/// Compacting starts once the context passes this share of the input limit. The rest is
/// room for the summary request and its answer, so 256k compacts at about 205k.
pub const AUTO_COMPACT_PERCENT: u64 = 80;

impl Agent {
    /// Compacts when the context has passed the automatic limit and there is something
    /// to work on: a queued message, or the running `turn`.
    pub async fn compact_if_needed(&self, turn: Option<&Turn>, inbox: &mut Inbox) -> Option<Stop> {
        let limit = self.input_limit();
        let conversation = match Conversation::load(&self.state, &self.thread_id).await {
            Ok(conversation) => conversation,
            Err(error) => {
                warn!("cannot check the context size: {error:#}");
                return None;
            }
        };
        let specs = self.tools.specs(&self.scope, &self.config.current());
        let working = turn.is_some() || conversation.has_queued();
        let full = conversation.tokens(&specs) >= limit / 100 * AUTO_COMPACT_PERCENT;
        if !working || !full || conversation.compactable_through().is_none() {
            return None;
        }
        self.compact(None, turn, inbox).await
    }

    /// Summarizes everything that is not queued. A failure is reported to the client and
    /// leaves the context as it was.
    pub async fn compact(
        &self,
        instructions: Option<&str>,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Option<Stop> {
        self.emit(ThreadEvent::CompactionStarted);
        match self.summarize(instructions, turn, inbox).await {
            Ok(Ok(compaction)) => {
                self.emit(ThreadEvent::CompactionFinished(compaction));
                self.report_tokens().await;
                None
            }
            Ok(Err(stop)) => Some(stop),
            Err(error) => {
                let error = format!("{error:#}");
                self.emit(ThreadEvent::CompactionFailed { error });
                None
            }
        }
    }

    async fn summarize(
        &self,
        instructions: Option<&str>,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Result<Result<Compaction, Stop>> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        let Some(through) = conversation.compactable_through() else {
            bail!("nothing to compact since the last summary");
        };
        let mut messages = conversation.chat_messages_through(through);
        messages.push(ChatMessage::User(self.instructions(instructions)?));
        let choice = self.model.current().context("provider is unavailable")?;
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &[],
            max_output_tokens: self.config.current().limits().max_output_tokens,
            effort: self.config.current().effort(),
        };
        let (output, generated) = match self.call_model(&choice, request, inbox).await {
            Ok(called) => called,
            Err(stop) => return Ok(Err(stop)),
        };
        if let Some(usage) = output.usage {
            let call = ModelCall {
                thread_id: self.thread_id.clone(),
                turn_id: turn.map(|turn| turn.turn_id.clone()),
                purpose: CallPurpose::Compact,
                compaction_id: conversation.compaction.as_ref().map(|c| c.compaction_id),
                model: choice.model.id.clone(),
                through_sequence: through,
            };
            self.state.record_usage(&call, usage).await?;
        }
        match generated {
            Err(stop) => return Ok(Err(stop)),
            Ok(result) => result?,
        };
        let summary = output.text.trim();
        if summary.is_empty() {
            bail!("the model wrote no summary");
        }
        let compaction = self.state.add_compaction(&self.thread_id, through, summary);
        Ok(Ok(compaction.await?))
    }

    /// COMPACT.md, or the built-in text when it is missing or blank.
    /// `extra` is what the user wrote after /compact.
    fn instructions(&self, extra: Option<&str>) -> Result<String> {
        let prompt = read_prompt(&self.compact_prompt)?;
        let mut instructions = prompt.unwrap_or_else(|| COMPACT_PROMPT_TEMPLATE.to_owned());
        if let Some(extra) = extra {
            instructions.push_str(&format!("\nAlso follow these instructions: {extra}\n"));
        }
        Ok(instructions)
    }
}
