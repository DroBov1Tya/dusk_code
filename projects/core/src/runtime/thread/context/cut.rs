//! Where a compaction may end its summary: queued messages stay, and a step's calls stay
//! with their results.

use super::Conversation;
use crate::state::{Message, MessageRole};

/// A message a summary may end on, and the tokens sent up to it, itself included.
struct Cut {
    sequence: u64,
    summarized: u64,
}

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
        let (cuts, total) = self.cuts()?;
        cuts.iter()
            .take_while(|cut| cut.summarized * 2 <= total)
            .last()
            .map(|cut| cut.sequence)
    }

    /// The newest message a compaction could cover while the newest messages, at most
    /// `keep_tokens` of them as requests send them, stay after the summary word for
    /// word. The cut keeps as much as fits, never between a step's calls and their
    /// results. `None` when no cut keeps a tail: everything fits in it, or the newest
    /// step alone does not.
    pub fn compactable_before(&self, keep_tokens: u64) -> Option<u64> {
        let (cuts, total) = self.cuts()?;
        let kept = |cut: &Cut| total - cut.summarized;
        let cut = cuts.iter().find(|cut| kept(cut) <= keep_tokens)?;
        // A cut before the first message summarizes nothing; one after the last keeps
        // nothing.
        (total > keep_tokens && kept(cut) > 0).then_some(cut.sequence)
    }

    /// The messages a summary may end on, oldest first, and the tokens of everything a
    /// compaction would summarize now. Never right before a result, nor right after a
    /// message that makes calls, whether or not its results are stored yet: every call
    /// the model sees keeps its result.
    fn cuts(&self) -> Option<(Vec<Cut>, u64)> {
        let through = self.compactable_through()?;
        let summarized: Vec<&Message> = self
            .messages
            .iter()
            .filter(|message| message.role != MessageRole::System && message.sequence <= through)
            .filter(|message| self.chat_message(message).is_some())
            .collect();
        let mut tokens = 0;
        let mut cuts = Vec::new();
        for (index, message) in summarized.iter().enumerate() {
            tokens += self.sent_tokens(message);
            let calls = !message.tool_calls.is_empty();
            let next = summarized.get(index + 1);
            if !calls && next.is_none_or(|next| next.role != MessageRole::Tool) {
                cuts.push(Cut {
                    sequence: message.sequence,
                    summarized: tokens,
                });
            }
        }
        Some((cuts, tokens))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::providers::ToolCall;

    /// A thread of steps after its prompt and the user's message, all answered: each step
    /// makes as many calls as it has result sizes, in tokens, and one with none answers.
    fn thread(steps: &[&[u64]]) -> Conversation {
        let mut messages = Vec::new();
        let mut add = |role, tokens, calls: Vec<ToolCall>, call: Option<String>| {
            let sequence = messages.len() as u64 + 1;
            messages.push(Message {
                message_id: format!("id-{sequence}"),
                thread_id: "thread".into(),
                sequence,
                role,
                content: String::new(),
                reasoning: None,
                tool_calls: calls,
                tool_call_id: call,
                artifacts: Vec::new(),
                from_thread_id: None,
                from_job_id: None,
                content_tokens: tokens,
                tool_calls_tokens: 0,
                reasoning_tokens: 0,
                created_at: 0,
            });
        };
        add(MessageRole::System, 10, Vec::new(), None);
        add(MessageRole::User, 10, Vec::new(), None);
        for (step, sizes) in steps.iter().enumerate() {
            let ids: Vec<String> = (0..sizes.len()).map(|n| format!("{step}-{n}")).collect();
            let calls = ids
                .iter()
                .map(|id| ToolCall {
                    id: id.clone(),
                    name: "run_command".into(),
                    arguments: json!({}),
                })
                .collect();
            add(MessageRole::Assistant, 10, calls, None);
            for (id, size) in ids.into_iter().zip(sizes.iter()) {
                add(MessageRole::Tool, *size, Vec::new(), Some(id));
            }
        }
        let answered_through = messages.len() as u64;
        Conversation {
            messages,
            compaction: None,
            answered_through,
            last_prompt: None,
            senders: HashMap::new(),
            stubs: HashMap::new(),
        }
    }

    /// What a cut at `through` keeps, and whether every kept result's call is kept too.
    fn kept(conversation: &Conversation, through: u64) -> (u64, bool) {
        let kept: Vec<&Message> = conversation
            .messages
            .iter()
            .filter(|message| message.sequence > through && message.role != MessageRole::System)
            .collect();
        let tokens = kept.iter().map(|m| conversation.sent_tokens(m)).sum();
        let calls: Vec<&str> = kept
            .iter()
            .flat_map(|message| message.tool_calls.iter().map(|call| call.id.as_str()))
            .collect();
        let paired = kept
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .all(|id| calls.contains(&id));
        (tokens, paired)
    }

    #[test]
    fn a_tail_keeps_whole_steps_up_to_its_size() {
        // 2 user; 3 calls, 4 result; 5 calls, 6-8 results; 9 calls, 10 result; 11 answer.
        let conversation = thread(&[&[100], &[300, 300, 300], &[50], &[]]);
        // The parallel step does not fit in 500, so it goes whole into the summary.
        assert_eq!(conversation.compactable_before(500), Some(8));
        assert_eq!(kept(&conversation, 8), (70, true));
        assert_eq!(conversation.compactable_before(1_000), Some(4));
        assert_eq!(conversation.compactable_before(1_095), Some(2));
        // Everything fits: nothing to summarize before a tail.
        assert_eq!(conversation.compactable_before(1_130), None);
        // The newest step alone is larger than the tail.
        assert_eq!(conversation.compactable_before(5), None);
        // The half summarizes at most half, as it did before the tail.
        assert_eq!(conversation.compactable_half(), Some(4));
    }

    #[test]
    fn a_turn_cut_off_after_its_calls_is_never_cut_there() {
        let mut conversation = thread(&[&[100], &[100]]);
        // The last results never arrived.
        conversation.messages.pop();
        let last = conversation.messages.last().unwrap().sequence;
        for keep in 1..200 {
            assert_ne!(conversation.compactable_before(keep), Some(last), "{keep}");
        }
        assert_eq!(conversation.compactable_before(30), Some(4));
    }

    proptest! {
        /// Whatever the steps and the tail, a cut keeps calls with their results, keeps
        /// something, summarizes something and keeps no more than the tail.
        #[test]
        fn a_cut_never_parts_a_call_from_its_result(
            steps in prop::collection::vec(prop::collection::vec(1u64..400, 0..4), 1..12),
            keep in 1u64..3_000,
        ) {
            let steps: Vec<&[u64]> = steps.iter().map(Vec::as_slice).collect();
            let conversation = thread(&steps);
            if let Some(through) = conversation.compactable_before(keep) {
                let (tokens, paired) = kept(&conversation, through);
                prop_assert!(paired);
                prop_assert!(tokens > 0 && tokens <= keep);
                prop_assert!(through >= 2);
            }
            if let Some(through) = conversation.compactable_half() {
                prop_assert!(kept(&conversation, through).1);
            }
        }
    }
}
