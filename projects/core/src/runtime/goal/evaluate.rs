//! The evaluator: after a turn, a separate model call reads the goal and the conversation
//! and says whether the goal is met. It runs no tool and reads no file, so it judges only
//! what the agent showed. Its answer is untrusted: anything that is not one of the three
//! verdicts is asked again a bounded number of times, then read as not met.

use anyhow::{Context, Result};
use dusk_protocol::text::clip_chars;
use tracing::warn;

use crate::{
    providers::{ChatMessage, ChatRequest, ModelOutput},
    runtime::{
        agent::{Agent, generation},
        thread::Conversation,
    },
    state::{Goal, GoalVerdict},
};

/// Calls before an answer that cannot be read counts as not met.
const TRIES: usize = 2;
/// Room for a reasoning model to think before its one line.
const ANSWER_TOKENS: u64 = 2048;
/// How much of the conversation the evaluator reads: the start, which holds the summary
/// of a compacted chat, and the newest part, where the evidence of the last turn is.
const HEAD_CHARS: usize = 8_000;
const TAIL_CHARS: usize = 52_000;
/// How much of one message or result it reads.
const MESSAGE_CHARS: usize = 4_000;
const REASON_CHARS: usize = 600;
const UNJUDGED: &str = "The evaluator could not judge this turn. Keep working, and show \
     evidence in the conversation that the goal holds.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgment {
    pub verdict: GoalVerdict,
    pub reason: String,
    /// The evaluator gave a verdict; false when every try failed or was unreadable and
    /// the turn is read as not met without one.
    pub judged: bool,
}

impl Agent {
    /// The verdict on `goal` after the turn that just ended.
    pub(crate) async fn judge_goal(&self, goal: &Goal) -> Judgment {
        for _ in 0..TRIES {
            match self.ask_evaluator(goal).await {
                Ok(Some(judgment)) => return judgment,
                Ok(None) => warn!("the goal evaluator gave no verdict"),
                Err(error) => warn!("cannot judge the goal: {error:#}"),
            }
        }
        Judgment {
            verdict: GoalVerdict::NotMet,
            reason: UNJUDGED.into(),
            judged: false,
        }
    }

    async fn ask_evaluator(&self, goal: &Goal) -> Result<Option<Judgment>> {
        let conversation = Conversation::load(&self.state, &self.thread_id, false).await?;
        let prompt = crate::release::resources::text("internal/GOAL_EVALUATOR.md")?;
        let asked = format!(
            "Goal: {}\nTurns judged before this one: {}\n\nConversation:\n{}",
            goal.text,
            goal.turns,
            digest(&conversation.chat_messages())
        );
        let messages = [ChatMessage::System(prompt), ChatMessage::User(asked)];
        let choice = self.model.current().context("provider is unavailable")?;
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &[],
            max_output_tokens: Some(ANSWER_TOKENS),
            effort: None,
            cache_key: None,
        };
        let idle = std::time::Duration::from_secs(
            self.config.current().limits().model_timeout_seconds.into(),
        );
        let mut output = ModelOutput::default();
        generation::generate(choice.provider.as_ref(), request, &mut output, idle, |_| {}).await?;
        Ok(parse(&output.text))
    }
}

/// The conversation as text, oldest first, cut to what the evaluator reads.
fn digest(messages: &[ChatMessage]) -> String {
    let mut text = String::new();
    for message in messages {
        let entry = match message {
            ChatMessage::System(_) => continue,
            ChatMessage::User(content) | ChatMessage::UserWithPictures { text: content, .. } => {
                format!("USER: {}\n", clip(content))
            }
            ChatMessage::Assistant { text, tool_calls } => {
                let mut entry = String::new();
                if !text.trim().is_empty() {
                    entry.push_str(&format!("AGENT: {}\n", clip(text)));
                }
                for call in tool_calls {
                    let arguments = clip_chars(&call.arguments.to_string(), 600, "...");
                    entry.push_str(&format!("AGENT CALLED {} {arguments}\n", call.name));
                }
                entry
            }
            ChatMessage::Tool { content, .. } => format!("RESULT: {}\n", clip(content)),
        };
        text.push_str(&entry);
    }
    let length = text.chars().count();
    if length <= HEAD_CHARS + TAIL_CHARS {
        return text;
    }
    let head: String = text.chars().take(HEAD_CHARS).collect();
    let tail: String = text.chars().skip(length - TAIL_CHARS).collect();
    format!("{head}\n[... the middle of the conversation is left out ...]\n{tail}")
}

fn clip(text: &str) -> String {
    clip_chars(text.trim(), MESSAGE_CHARS, "...")
}

/// The verdict and its reason from the evaluator's answer; `None` when the answer is not
/// one of the three verdicts.
pub fn parse(answer: &str) -> Option<Judgment> {
    let line = answer
        .lines()
        .map(|line| line.trim().trim_start_matches(['*', '#', '`', '-', ' ']))
        .find(|line| !line.is_empty())?;
    let (verdict, rest) = [
        ("NOT_MET", GoalVerdict::NotMet),
        ("NOT MET", GoalVerdict::NotMet),
        ("MET", GoalVerdict::Met),
        ("IMPOSSIBLE", GoalVerdict::Impossible),
    ]
    .into_iter()
    .find_map(|(word, verdict)| {
        let head = line.get(..word.len())?;
        head.eq_ignore_ascii_case(word)
            .then(|| (verdict, &line[word.len()..]))
    })?;
    // `METHOD`, `MET_ALREADY` and the like are not a verdict.
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let reason = rest
        .trim_start_matches(['*', '`', ':', '-', ' ', '\u{2014}'])
        .trim();
    let reason = match reason.is_empty() {
        true => match verdict {
            GoalVerdict::NotMet => UNJUDGED.to_owned(),
            GoalVerdict::Met | GoalVerdict::Impossible => "no reason given".to_owned(),
        },
        false => clip_chars(reason, REASON_CHARS, "..."),
    };
    Some(Judgment {
        verdict,
        reason,
        judged: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(answer: &str) -> Option<(GoalVerdict, String)> {
        parse(answer).map(|judgment| (judgment.verdict, judgment.reason))
    }

    #[test]
    fn the_three_verdicts_are_read_with_their_reason() {
        assert_eq!(
            verdict("MET: the tests pass"),
            Some((GoalVerdict::Met, "the tests pass".into()))
        );
        assert_eq!(
            verdict("\n**NOT_MET** - no test was run\nmore"),
            Some((GoalVerdict::NotMet, "no test was run".into()))
        );
        assert_eq!(
            verdict("not met: build fails"),
            Some((GoalVerdict::NotMet, "build fails".into()))
        );
        assert_eq!(
            verdict("IMPOSSIBLE \u{2014} the file does not exist"),
            Some((GoalVerdict::Impossible, "the file does not exist".into()))
        );
        assert_eq!(
            verdict("MET"),
            Some((GoalVerdict::Met, "no reason given".into()))
        );
    }

    #[test]
    fn anything_else_is_no_verdict() {
        for answer in [
            "",
            "  \n ",
            "The goal is met.",
            "METHOD: none",
            "MET_ALREADY",
            "IMPOSSIBLY hard",
            "ALLOW: fine",
            "Sure! MET: done",
            "\u{131}mpossible: dotless",
        ] {
            assert_eq!(parse(answer), None, "{answer:?}");
        }
    }

    #[test]
    fn a_long_reason_is_cut() {
        let answer = format!("NOT_MET: {}", "x".repeat(5_000));
        let judgment = parse(&answer).unwrap();
        assert_eq!(judgment.reason.chars().count(), REASON_CHARS + 3);
    }

    #[test]
    fn a_long_conversation_keeps_its_start_and_its_end() {
        let mut messages = vec![ChatMessage::System("mode".into())];
        messages.push(ChatMessage::User("summary of the start".into()));
        for _ in 0..40 {
            messages.push(ChatMessage::Tool {
                call_id: "c".into(),
                content: "y".repeat(10_000),
                pictures: Vec::new(),
            });
        }
        messages.push(ChatMessage::Assistant {
            text: "all tests pass".into(),
            tool_calls: Vec::new(),
        });
        let text = digest(&messages);
        assert!(text.starts_with("USER: summary of the start"));
        assert!(text.trim_end().ends_with("AGENT: all tests pass"));
        assert!(!text.contains("mode"));
        assert!(text.chars().count() < HEAD_CHARS + TAIL_CHARS + 100);
    }
}
