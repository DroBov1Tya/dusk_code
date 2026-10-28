//! A thread as the model sees it: its system messages, the newest compaction summary and
//! the messages after it, and which of them are still waiting for an answer.

use anyhow::Result;

use crate::{
    providers::{ChatMessage, ToolSpec},
    state::{Compaction, LastPrompt, Message, MessageRole, State, estimate_tokens},
};

/// Introduces a compaction summary to the model.
const SUMMARY_INTRO: &str = "The earlier part of this conversation was replaced by this \
summary. Its messages can still be read with the read_history tool.\n\n";

pub(super) struct Conversation {
    /// The system messages and the messages after the compaction, in order.
    pub(super) messages: Vec<Message>,
    pub(super) compaction: Option<Compaction>,
    /// Sequence of the newest message a turn has taken; user messages after it are queued.
    pub(super) answered_through: u64,
    /// The provider's count of the newest step over this context.
    pub(super) last_prompt: Option<LastPrompt>,
}

impl Conversation {
    pub(super) async fn load(state: &State, thread_id: &str) -> Result<Self> {
        let compaction = state.latest_compaction(thread_id).await?;
        let after = compaction
            .as_ref()
            .map_or(0, |compaction| compaction.through_sequence);
        let compaction_id = compaction
            .as_ref()
            .map(|compaction| compaction.compaction_id);
        Ok(Self {
            messages: state.context_messages(thread_id, after).await?,
            answered_through: state.answered_through(thread_id).await?,
            last_prompt: state.last_prompt(thread_id, compaction_id).await?,
            compaction,
        })
    }

    pub(super) fn has_queued(&self) -> bool {
        self.messages.iter().any(|message| self.is_queued(message))
    }

    /// IDs, timestamps and other metadata stay in the core and never reach the model.
    pub(super) fn chat_messages(&self) -> Vec<ChatMessage> {
        self.chat_messages_through(u64::MAX)
    }

    /// The request a compaction summarizes: everything up to `through`.
    pub(super) fn chat_messages_through(&self, through: u64) -> Vec<ChatMessage> {
        let (system, rest): (Vec<_>, Vec<_>) = self
            .messages
            .iter()
            .filter(|message| message.sequence <= through)
            .partition(|message| message.role == MessageRole::System);
        let summary = self
            .compaction
            .as_ref()
            .map(|compaction| ChatMessage::User(format!("{SUMMARY_INTRO}{}", compaction.summary)));
        let system = system.into_iter().filter_map(chat_message);
        let rest = rest.into_iter().filter_map(chat_message);
        system.chain(summary).chain(rest).collect()
    }

    /// The newest message `chat_messages` covers, the summary included.
    pub(super) fn newest_sequence(&self) -> u64 {
        let newest = self.messages.last().map_or(0, |message| message.sequence);
        newest.max(self.covered_through())
    }

    /// The newest message a compaction could summarize now: queued messages wait for
    /// their turn, so they stay. `None` when nothing is new since the last compaction.
    pub(super) fn compactable_through(&self) -> Option<u64> {
        self.messages
            .iter()
            .filter(|message| message.role != MessageRole::System && !self.is_queued(message))
            .map(|message| message.sequence)
            .max()
    }

    /// Size of a request with these messages and tools. The provider's count of the last
    /// request is exact, so only what came after it is estimated; estimates alone tend
    /// to miss the chat template markup.
    pub(super) fn tokens(&self, specs: &[ToolSpec]) -> u64 {
        let Some(last) = self.last_prompt else {
            let summary = self.compaction.as_ref().map_or(0, |c| c.summary_tokens);
            return self.estimated_after(0) + summary + spec_tokens(specs);
        };
        last.prompt_tokens + self.estimated_after(last.through_sequence)
    }

    fn covered_through(&self) -> u64 {
        self.compaction
            .as_ref()
            .map_or(0, |compaction| compaction.through_sequence)
    }

    fn is_queued(&self, message: &Message) -> bool {
        message.role == MessageRole::User && message.sequence > self.answered_through
    }

    fn estimated_after(&self, sequence: u64) -> u64 {
        self.messages
            .iter()
            .filter(|message| message.sequence > sequence && chat_message(message).is_some())
            .map(Message::context_tokens)
            .sum()
    }
}

/// Estimated size of the tool definitions a request carries.
pub(super) fn spec_tokens(specs: &[ToolSpec]) -> u64 {
    specs
        .iter()
        .map(|spec| {
            estimate_tokens(&spec.name)
                + estimate_tokens(&spec.description)
                + estimate_tokens(&spec.parameters.to_string())
        })
        .sum()
}

fn chat_message(message: &Message) -> Option<ChatMessage> {
    let content = message.content.clone();
    match message.role {
        MessageRole::System => Some(ChatMessage::System(content)),
        MessageRole::User => Some(ChatMessage::User(content)),
        MessageRole::Assistant => Some(ChatMessage::Assistant {
            text: content,
            tool_calls: message.tool_calls.clone(),
        }),
        MessageRole::Tool => Some(ChatMessage::Tool {
            call_id: message.tool_call_id.clone()?,
            content,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sequence: u64, role: MessageRole, content: &str) -> Message {
        Message {
            message_id: format!("id-{sequence}"),
            thread_id: "thread".into(),
            sequence,
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: (role == MessageRole::Tool).then(|| "call".into()),
            content_tokens: 5,
            tool_calls_tokens: 0,
            reasoning_tokens: 0,
            created_at: 0,
        }
    }

    #[test]
    fn sends_role_text_and_tool_links_only() {
        let conversation = Conversation {
            messages: vec![
                message(1, MessageRole::System, "prompt"),
                message(2, MessageRole::User, "question"),
                message(3, MessageRole::Assistant, "answer"),
                message(4, MessageRole::Tool, "tool output"),
                message(5, MessageRole::User, "follow-up"),
            ],
            compaction: None,
            answered_through: 2,
            last_prompt: None,
        };
        assert_eq!(
            conversation.chat_messages(),
            [
                ChatMessage::System("prompt".into()),
                ChatMessage::User("question".into()),
                ChatMessage::Assistant {
                    text: "answer".into(),
                    tool_calls: Vec::new()
                },
                ChatMessage::Tool {
                    call_id: "call".into(),
                    content: "tool output".into()
                },
                ChatMessage::User("follow-up".into()),
            ]
        );
        assert_eq!(conversation.tokens(&[]), 25);
        assert!(conversation.has_queued(), "the follow-up waits");
    }

    #[test]
    fn the_last_prompt_count_replaces_estimates_up_to_its_message() {
        let conversation = Conversation {
            messages: (1..=4)
                .map(|sequence| message(sequence, MessageRole::User, "text"))
                .collect(),
            compaction: None,
            answered_through: 0,
            last_prompt: Some(LastPrompt {
                prompt_tokens: 100,
                through_sequence: 3,
            }),
        };
        let spec = ToolSpec {
            name: "tool".into(),
            description: "Does things.".into(),
            parameters: serde_json::json!({}),
        };

        // The tool definition is inside the provider's count already.
        assert_eq!(conversation.tokens(&[spec]), 100 + 5);
    }

    #[test]
    fn a_summary_follows_the_system_messages_and_queued_messages_stay() {
        let conversation = Conversation {
            messages: vec![
                message(1, MessageRole::System, "prompt"),
                message(5, MessageRole::Assistant, "done"),
                message(6, MessageRole::User, "next"),
            ],
            compaction: Some(Compaction {
                compaction_id: 1,
                thread_id: "thread".into(),
                through_sequence: 4,
                summary: "so far".into(),
                summary_tokens: 2,
                reason: crate::state::CompactionReason::Manual,
                created_at: 0,
            }),
            answered_through: 5,
            last_prompt: None,
        };

        let messages = conversation.chat_messages();
        assert_eq!(messages[0], ChatMessage::System("prompt".into()));
        assert_eq!(
            messages[1],
            ChatMessage::User(format!("{SUMMARY_INTRO}so far"))
        );
        assert_eq!(messages.len(), 4);
        assert_eq!(conversation.newest_sequence(), 6);
        assert_eq!(conversation.compactable_through(), Some(5), "6 is queued");
        assert_eq!(conversation.tokens(&[]), 5 + 5 + 2 + 5);
    }
}
