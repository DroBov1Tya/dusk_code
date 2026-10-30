//! Compaction: the model summarizes the context so far, and the summary takes the place
//! of the older messages. It runs on `/compact`, and on its own before the context
//! outgrows the model window. `/fork` writes a fuller summary the same way, for a new
//! thread to start from.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tracing::warn;

use super::{Agent, Inbox, Stop};
use crate::runtime::{chat::Conversation, system::ThreadEvent};
use crate::{
    catalog::modes,
    providers::{ChatMessage, ChatRequest},
    runtime::chat::thread_start::{NO_PROMPT, read_prompt},
    state::{CallPurpose, Compaction, ModelCall, Turn},
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
        let prompt =
            self.instructions(&self.compact_prompt, "internal/COMPACT.md", instructions)?;
        let (summary, through) = match self.write_summary(prompt, turn, inbox).await? {
            Ok(written) => written,
            Err(stop) => return Ok(Err(stop)),
        };
        let compaction = self
            .state
            .add_compaction(&self.thread_id, through, &summary);
        let compaction = compaction.await?;
        // The context starts over here, so a chat takes up the prompt of its mode now.
        if self.depth == 0 {
            let mode = self.config.current().mode().to_owned();
            let prompt = modes::prompt(&self.modes_dir, &mode)?;
            let prompt = prompt.as_deref().unwrap_or(NO_PROMPT);
            self.state
                .add_system_message(&self.thread_id, prompt)
                .await?;
        }
        Ok(Ok(compaction))
    }

    /// `/fork`: describes the thread in detail for a new thread to start from. This
    /// thread is left as it was. A failure is reported to the client.
    pub async fn fork(&self, instructions: Option<&str>, inbox: &mut Inbox) -> Option<Stop> {
        let written = async {
            let prompt = self.instructions(&self.fork_prompt, "internal/FORK.md", instructions)?;
            self.write_summary(prompt, None, inbox).await
        };
        match written.await {
            Ok(Ok((summary, _))) => {
                self.emit(ThreadEvent::Forked { summary });
                None
            }
            Ok(Err(stop)) => Some(stop),
            Err(error) => {
                let reason = format!("cannot fork: {error:#}");
                self.emit(ThreadEvent::Rejected { reason });
                None
            }
        }
    }

    /// Asks the model to summarize everything that is not queued with `prompt`. Returns
    /// the summary and the newest message it covers.
    async fn write_summary(
        &self,
        prompt: String,
        turn: Option<&Turn>,
        inbox: &mut Inbox,
    ) -> Result<Result<(String, u64), Stop>> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        let Some(through) = conversation.compactable_through() else {
            bail!("nothing to summarize since the last summary");
        };
        let mut messages = conversation.chat_messages_through(through);
        messages.push(ChatMessage::User(prompt));
        let choice = self.model.current().context("provider is unavailable")?;
        let config = self.config.current();
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &[],
            max_output_tokens: config.limits().max_output_tokens,
            effort: config.effort(),
        };
        let (output, generated) = match self.call_model(&choice, request, &config, inbox).await {
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
        Ok(Ok((summary.to_owned(), through)))
    }

    /// The editable prompt, or its local resource when it is missing or blank. `extra` is
    /// what the user wrote after the command.
    fn instructions(&self, file: &Path, builtin: &str, extra: Option<&str>) -> Result<String> {
        let prompt = read_prompt(file)?;
        let mut instructions = match prompt {
            Some(prompt) => prompt,
            None => crate::release::resources::text(builtin)?,
        };
        if let Some(extra) = extra {
            instructions.push_str(&format!("\nAlso follow these instructions: {extra}\n"));
        }
        Ok(instructions)
    }
}
