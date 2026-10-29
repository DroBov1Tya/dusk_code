//! Naming a thread so clients can list it. The first message gives a stand-in title at
//! once; after the first turn the model names the thread from that message.

use anyhow::{Context, Result};
use tracing::warn;

use super::{
    agent::Agent,
    generation,
    inbox::{Inbox, Stop},
    thread_runner::ThreadEvent,
};
use crate::{
    providers::{ChatMessage, ChatRequest, ModelOutput},
    state::{CallPurpose, Message, MessageRole, ModelCall},
};

/// The stand-in title is the message cut to this many characters.
const FALLBACK_CHARS: usize = 35;
const MAX_TITLE_CHARS: usize = 80;
/// How much of the first message the model reads.
const MAX_PROMPT_CHARS: usize = 2000;
/// Room for a reasoning model to think before it names the thread.
const TITLE_OUTPUT_TOKENS: u64 = 4096;
const TITLE_PROMPT: &str = "Write a title for a chat that starts with the message below: at \
most six words, in the language of the message, without quotes or a final period. Answer \
with the title only.\n\nMessage:\n";

impl Agent {
    /// Gives a thread without a title the start of its first message.
    pub async fn title_from_first_message(&self, message: &Message) -> Result<()> {
        let thread = self.state.load_thread(&self.thread_id).await?;
        if thread.context("thread is gone")?.title.is_some() {
            return Ok(());
        }
        self.set_title(fallback_title(&message.content)).await
    }

    /// After the thread's first turn, asks the model for a name. Failing to get one only
    /// keeps the stand-in, so it is logged and not reported.
    pub async fn name_thread(&self, inbox: &mut Inbox) -> Option<Stop> {
        match self.ask_for_title(inbox).await {
            Ok(Ok(())) => None,
            Ok(Err(stop)) => Some(stop),
            Err(error) => {
                warn!("cannot name thread {}: {error:#}", self.thread_id);
                None
            }
        }
    }

    async fn ask_for_title(&self, inbox: &mut Inbox) -> Result<Result<(), Stop>> {
        // A subagent's thread keeps the start of its task as its name: nobody browses it.
        if self.depth > 0 || self.state.count_turns(&self.thread_id).await? != 1 {
            return Ok(Ok(()));
        }
        let messages = self.state.messages_from(&self.thread_id, 1, 50).await?;
        let first = messages
            .iter()
            .find(|message| message.role == MessageRole::User)
            .context("the thread has no user message")?;
        let prompt = format!("{TITLE_PROMPT}{}", clip(&first.content, MAX_PROMPT_CHARS));
        let max_output = self.config.current().limits().max_output_tokens;
        let choice = self.model.current().context("provider is unavailable")?;
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &[ChatMessage::User(prompt)],
            tools: &[],
            max_output_tokens: max_output.min(TITLE_OUTPUT_TOKENS),
        };
        // Not shown to the client: the title arrives as one event.
        let mut output = ModelOutput::default();
        let generated = generation::generate(&*choice.provider, request, &mut output, |_| {});
        let generated = match inbox.race(generated).await {
            Ok(generated) => generated,
            Err(stop) => return Ok(Err(stop)),
        };
        if let Some(usage) = output.usage {
            let call = ModelCall {
                thread_id: self.thread_id.clone(),
                turn_id: None,
                purpose: CallPurpose::Title,
                compaction_id: None,
                model: choice.model.id.clone(),
                through_sequence: first.sequence,
            };
            self.state.record_usage(&call, usage).await?;
        }
        generated?;
        let title = clean_title(&output.text).context("the model gave no title")?;
        self.set_title(title).await?;
        Ok(Ok(()))
    }

    async fn set_title(&self, title: String) -> Result<()> {
        self.state.set_title(&self.thread_id, &title).await?;
        self.emit(ThreadEvent::TitleChanged { title });
        Ok(())
    }
}

/// The message on one line, cut with an ellipsis when it is long.
pub fn fallback_title(text: &str) -> String {
    let line = one_line(text);
    match line.char_indices().nth(FALLBACK_CHARS) {
        Some((end, _)) => format!("{}...", line[..end].trim_end()),
        None => line,
    }
}

/// The first line of the answer without the quotes and markup models like to add.
fn clean_title(answer: &str) -> Option<String> {
    let line = answer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let decoration = |c: char| matches!(c, '"' | '\'' | '`' | '*' | '#' | '.') || c.is_whitespace();
    let title = clip(&one_line(line.trim_matches(decoration)), MAX_TITLE_CHARS);
    (!title.is_empty()).then_some(title)
}

/// Whitespace and control characters collapse to single spaces.
fn one_line(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}
