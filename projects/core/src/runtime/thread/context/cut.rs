//! Where a compaction may end its summary: queued messages stay, and a step's calls stay
//! with their results.

use super::Conversation;
use crate::state::{Message, MessageRole};

impl Conversation {
    /// The newest message a compaction could summarize now: queued messages wait for
    /// their turn, so they stay. `None` when nothing is new since the last compaction.
    pub fn compactable_through(&self) -> Option<u64> {
        self.messages
            .iter()
            .filter(|message| message.role != MessageRole::System && !self.is_queued(message))
            .map(|message| message.sequence)
            .max()
    }

    /// The newest message a compaction of the older half could cover: at or below half
    /// of the tokens a compaction would summarize now, and never between a step's calls
    /// and their results, which stay together. `None` when no such message is there.
    pub fn compactable_half(&self) -> Option<u64> {
        let through = self.compactable_through()?;
        let summarized: Vec<&Message> = self
            .messages
            .iter()
            .filter(|message| message.role != MessageRole::System && message.sequence <= through)
            .filter(|message| self.chat_message(message).is_some())
            .collect();
        let total: u64 = summarized
            .iter()
            .map(|message| self.sent_tokens(message))
            .sum();
        let mut tokens = 0;
        let mut half = None;
        for (index, message) in summarized.iter().enumerate() {
            tokens += self.sent_tokens(message);
            if tokens * 2 > total {
                break;
            }
            let next = summarized.get(index + 1);
            if next.is_none_or(|next| next.role != MessageRole::Tool) {
                half = Some(message.sequence);
            }
        }
        half
    }
}
