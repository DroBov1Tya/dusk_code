//! Compaction: the model summarizes the context so far, and the summary takes the place
//! of the older messages. It runs on `/compact`, and on its own before the context
//! outgrows the model window.

use anyhow::{Result, bail};
use tracing::warn;

use super::{
    agent::Agent,
    context::Conversation,
    inbox::{Inbox, Stop},
    thread_runner::ThreadEvent,
};
use crate::{
    init::COMPACT_PROMPT_TEMPLATE,
    providers::{ChatMessage, ChatRequest},
    state::{CallPurpose, Compaction, CompactionReason, ModelCall, Turn},
    thread_start::read_prompt,
};

/// Compacting starts once the context passes this share of the model window. The rest
/// is room for the summary request and its answer, so 256k compacts at about 205k.
const AUTO_COMPACT_PERCENT: u64 = 80;

impl Agent {
    /// Compacts when the context has passed the automatic limit and there is something
    /// to work on: a queued message, or the running `turn`.
    pub(super) async fn compact_if_needed(
        &self,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Option<Stop> {
        let window = self.model.context_window?;
        let conversation = match Conversation::load(&self.state, &self.thread_id).await {
            Ok(conversation) => conversation,
            Err(error) => {
                warn!("cannot check the context size: {error:#}");
                return None;
            }
        };
        let specs = self.tools.specs(&self.scope, &self.config.current());
        let working = turn.is_some() || conversation.has_queued();
        let full = conversation.tokens(&specs) >= window / 100 * AUTO_COMPACT_PERCENT;
        if !working || !full || conversation.compactable_through().is_none() {
            return None;
        }
        self.compact(CompactionReason::Auto, None, turn, inbox)
            .await
    }

    /// Summarizes everything that is not queued. A failure is reported to the client and
    /// leaves the context as it was.
    pub(super) async fn compact(
        &self,
        reason: CompactionReason,
        focus: Option<&str>,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Option<Stop> {
        self.emit(ThreadEvent::CompactionStarted { reason });
        match self.summarize(reason, focus, turn, inbox).await {
            Ok(Ok(compaction)) => {
                self.emit(ThreadEvent::CompactionFinished(compaction));
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
        reason: CompactionReason,
        focus: Option<&str>,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Result<Result<Compaction, Stop>> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        let Some(through) = conversation.compactable_through() else {
            bail!("nothing to compact since the last summary");
        };
        let mut messages = conversation.chat_messages_through(through);
        messages.push(ChatMessage::User(self.instructions(focus)?));
        let request = ChatRequest {
            model: &self.model.id,
            messages: &messages,
            tools: &[],
        };
        let (output, generated) = match self.call_model(request, inbox).await {
            Ok(called) => called,
            Err(stop) => return Ok(Err(stop)),
        };
        if let Some(usage) = output.usage {
            let call = ModelCall {
                thread_id: self.thread_id.clone(),
                turn_id: turn.map(|turn| turn.turn_id.clone()),
                purpose: CallPurpose::Compact,
                compaction_id: conversation.compaction.as_ref().map(|c| c.compaction_id),
                model: self.model.id.clone(),
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
        let compaction = self
            .state
            .add_compaction(&self.thread_id, through, summary, reason);
        Ok(Ok(compaction.await?))
    }

    /// COMPACT.md, or the built-in text when it is missing or blank.
    fn instructions(&self, focus: Option<&str>) -> Result<String> {
        let prompt = read_prompt(&self.compact_prompt)?;
        let mut instructions = prompt.unwrap_or_else(|| COMPACT_PROMPT_TEMPLATE.to_owned());
        if let Some(focus) = focus {
            instructions.push_str(&format!("\nFocus the summary on: {focus}\n"));
        }
        Ok(instructions)
    }
}
