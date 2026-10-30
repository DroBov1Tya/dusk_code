//! A thread as the model sees it: its system messages, the newest compaction summary and
//! the messages after it, and which of them are still waiting for an answer.

use std::collections::HashMap;

use anyhow::Result;

use crate::{
    providers::{ChatMessage, ToolSpec},
    state::{Compaction, LastPrompt, Message, MessageRole, State, estimate_tokens},
    thread_start::{FORK_INTRO, NO_PROMPT, display_name},
};

/// Introduces a compaction summary to the model.
const SUMMARY_INTRO: &str = "The earlier part of this conversation was replaced by this \
summary. Its messages can still be read with the read_history tool.\n\n";

pub struct Conversation {
    /// The system messages and the messages after the compaction, in order.
    pub messages: Vec<Message>,
    pub compaction: Option<Compaction>,
    /// Sequence of the newest message a turn has taken; user messages after it are queued.
    pub answered_through: u64,
    /// The provider's count of the newest step over this context.
    pub last_prompt: Option<LastPrompt>,
    /// Who wrote the messages other agents sent, by thread: an agent's name, or the chat
    /// that asked.
    pub senders: HashMap<String, String>,
}

impl Conversation {
    pub async fn load(state: &State, thread_id: &str) -> Result<Self> {
        let compaction = state.latest_compaction(thread_id).await?;
        let after = compaction
            .as_ref()
            .map_or(0, |compaction| compaction.through_sequence);
        let compaction_id = compaction
            .as_ref()
            .map(|compaction| compaction.compaction_id);
        let messages = state.context_messages(thread_id, after).await?;
        let mut senders = HashMap::new();
        for from in messages
            .iter()
            .filter_map(|message| message.from_thread_id.as_ref())
        {
            if senders.contains_key(from) {
                continue;
            }
            let thread = state.load_thread(from).await?;
            let sender = match thread {
                Some(thread) if thread.parent_thread_id.is_some() => display_name(&thread),
                _ => "the chat that asked you".to_owned(),
            };
            senders.insert(from.clone(), sender);
        }
        Ok(Self {
            messages,
            answered_through: state.answered_through(thread_id).await?,
            last_prompt: state.last_prompt(thread_id, compaction_id).await?,
            compaction,
            senders,
        })
    }

    /// The system messages as the model gets them, up to `through`: the newest prompt
    /// first, then each fork summary. A thread starts with its prompt and a compaction
    /// of a chat adds the prompt of the mode it is in then, so the context changes only
    /// at those points and a provider's cache of it holds in between.
    pub fn system_texts(&self, through: u64) -> Vec<String> {
        let (forks, prompts): (Vec<&Message>, Vec<&Message>) = self
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::System && message.sequence <= through)
            .partition(|message| message.content.starts_with(FORK_INTRO));
        let prompt = prompts
            .last()
            .map(|message| message.content.clone())
            .filter(|prompt| prompt != NO_PROMPT);
        let forks = forks.into_iter().map(|message| message.content.clone());
        prompt.into_iter().chain(forks).collect()
    }

    pub fn has_queued(&self) -> bool {
        self.messages.iter().any(|message| self.is_queued(message))
    }

    /// IDs, timestamps and other metadata stay in the core and never reach the model.
    pub fn chat_messages(&self) -> Vec<ChatMessage> {
        self.chat_messages_through(u64::MAX)
    }

    /// The request a compaction summarizes: everything up to `through`.
    pub fn chat_messages_through(&self, through: u64) -> Vec<ChatMessage> {
        let rest = self
            .messages
            .iter()
            .filter(|message| message.sequence <= through && message.role != MessageRole::System);
        let summary = self
            .compaction
            .as_ref()
            .map(|compaction| ChatMessage::User(format!("{SUMMARY_INTRO}{}", compaction.summary)));
        let system = self
            .system_texts(through)
            .into_iter()
            .map(ChatMessage::System);
        let rest = rest.filter_map(|message| chat_message(message, &self.senders));
        system.chain(summary).chain(rest).collect()
    }

    /// The newest message `chat_messages` covers, the summary included.
    pub fn newest_sequence(&self) -> u64 {
        let newest = self.messages.last().map_or(0, |message| message.sequence);
        newest.max(self.covered_through())
    }

    /// The newest message a compaction could summarize now: queued messages wait for
    /// their turn, so they stay. `None` when nothing is new since the last compaction.
    pub fn compactable_through(&self) -> Option<u64> {
        self.messages
            .iter()
            .filter(|message| message.role != MessageRole::System && !self.is_queued(message))
            .map(|message| message.sequence)
            .max()
    }

    /// Size of a request with these messages and tools. The provider's count of the last
    /// request is exact, so only what came after it is estimated; estimates alone tend
    /// to miss the chat template markup.
    pub fn tokens(&self, specs: &[ToolSpec]) -> u64 {
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
            .filter(|message| {
                message.sequence > sequence && chat_message(message, &self.senders).is_some()
            })
            .map(Message::context_tokens)
            .sum()
    }
}

/// Estimated size of the tool definitions a request carries.
pub fn spec_tokens(specs: &[ToolSpec]) -> u64 {
    specs
        .iter()
        .map(|spec| {
            estimate_tokens(&spec.name)
                + estimate_tokens(&spec.description)
                + estimate_tokens(&spec.parameters.to_string())
        })
        .sum()
}

fn chat_message(message: &Message, senders: &HashMap<String, String>) -> Option<ChatMessage> {
    let content = message.content.clone();
    match message.role {
        MessageRole::System => Some(ChatMessage::System(content)),
        // The model learns who wrote it; thread IDs are how agents address each other.
        MessageRole::User => Some(ChatMessage::User(match &message.from_thread_id {
            Some(from) => {
                let sender = senders.get(from).map_or("an agent", String::as_str);
                format!("[message from {sender}, thread {from}]\n{content}")
            }
            None => content,
        })),
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
            from_thread_id: None,
            content_tokens: 5,
            tool_calls_tokens: 0,
            reasoning_tokens: 0,
            created_at: 0,
        }
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
            }),
            answered_through: 5,
            last_prompt: None,
            senders: HashMap::new(),
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

    #[test]
    fn the_newest_prompt_leads_and_fork_summaries_stay() {
        let fork = format!("{FORK_INTRO}what came before");
        let conversation = Conversation {
            messages: vec![
                message(1, MessageRole::System, "main prompt"),
                message(2, MessageRole::System, &fork),
                message(3, MessageRole::User, "hi"),
                message(4, MessageRole::System, "plan prompt"),
            ],
            compaction: None,
            answered_through: 3,
            last_prompt: None,
            senders: HashMap::new(),
        };
        assert_eq!(
            conversation.system_texts(u64::MAX),
            ["plan prompt".to_owned(), fork.clone()]
        );
        assert_eq!(
            conversation.system_texts(3),
            ["main prompt".to_owned(), fork]
        );
    }
}
