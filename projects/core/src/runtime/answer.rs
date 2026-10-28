//! One model call: streams the answer to a conversation into a caller-owned `Answer`.

use anyhow::{Result, ensure};

use crate::providers::{ChatDelta, ChatMessage, ChatStream, ModelProvider};

/// Bounds memory for a provider that never finishes; reasoning counts too.
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;

/// The model output of one turn, split by kind.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Answer {
    pub(super) text: String,
    pub(super) reasoning: String,
}

/// The answer is owned by the caller so the text written so far survives when the caller
/// stops this future early.
pub(super) async fn generate<P: ModelProvider>(
    provider: &P,
    model: &str,
    messages: &[ChatMessage],
    answer: &mut Answer,
    mut on_delta: impl FnMut(&ChatDelta) + Send,
) -> Result<()> {
    let mut stream = provider.stream_chat(model, messages).await?;
    while let Some(delta) = stream.next_delta().await? {
        answer.push(&delta)?;
        on_delta(&delta);
    }
    ensure!(
        !answer.text.trim().is_empty(),
        "model returned an empty answer"
    );
    Ok(())
}

impl Answer {
    fn push(&mut self, delta: &ChatDelta) -> Result<()> {
        match delta {
            ChatDelta::Reasoning(text) => self.reasoning.push_str(text),
            ChatDelta::Text(text) => self.text.push_str(text),
        }
        ensure!(
            self.text.len() + self.reasoning.len() <= MAX_ANSWER_BYTES,
            "model answer is too large"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_reasoning_apart_from_text() {
        let mut answer = Answer::default();
        for delta in [
            ChatDelta::Reasoning("think".into()),
            ChatDelta::Text("Hel".into()),
            ChatDelta::Text("lo".into()),
        ] {
            answer.push(&delta).unwrap();
        }
        assert_eq!(answer.text, "Hello");
        assert_eq!(answer.reasoning, "think");
    }

    #[test]
    fn oversized_answer_is_an_error() {
        let mut answer = Answer::default();
        let half = "a".repeat(MAX_ANSWER_BYTES / 2 + 1);
        answer.push(&ChatDelta::Reasoning(half.clone())).unwrap();
        assert!(answer.push(&ChatDelta::Text(half)).is_err());
    }
}
