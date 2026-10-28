//! Trusted agent runtime: runs a queued turn against a model and streams the answer.

mod context;

use anyhow::{Result, ensure};

/// Bounds memory for a provider that never finishes; reasoning counts too.
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;

use crate::{
    providers::{ChatDelta, ChatStream, Model, Provider},
    receivers::{AgentOutcome, AgentReceipt, AgentReceiver, AgentResponse, AgentTask},
    state::{State, Turn},
};

pub struct AgentRuntime<'a> {
    receiver: AgentReceiver<'a>,
    provider: &'a Provider,
    model: Model,
}

impl<'a> AgentRuntime<'a> {
    pub fn new(
        state: &'a State,
        agent_id: impl Into<String>,
        provider: &'a Provider,
        model: Model,
    ) -> Result<Self> {
        Ok(Self {
            receiver: AgentReceiver::new(state, agent_id)?,
            provider,
            model,
        })
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Starts the turn, sends the conversation text to the model and reports each delta
    /// to `on_delta`. Model failures are recorded on the turn, not returned as errors.
    pub async fn run_turn(
        &self,
        turn: &Turn,
        mut on_delta: impl FnMut(&ChatDelta) + Send,
    ) -> Result<AgentReceipt> {
        let task = self
            .receiver
            .start_turn(&turn.turn_id, turn.attempt)
            .await?;
        let outcome = match self.answer(&task, &mut on_delta).await {
            Ok(content) => AgentOutcome::Completed {
                content,
                attachments: Vec::new(),
            },
            Err(error) => AgentOutcome::Failed {
                error_message: format!("{error:#}"),
            },
        };
        self.receiver
            .submit_response(AgentResponse {
                turn_id: task.turn.turn_id,
                expected_attempt: task.turn.attempt,
                outcome,
            })
            .await
    }

    async fn answer(
        &self,
        task: &AgentTask,
        on_delta: &mut (impl FnMut(&ChatDelta) + Send),
    ) -> Result<String> {
        let conversation = self.receiver.conversation(task).await?;
        let messages = context::chat_messages(&conversation);
        let stream = self.provider.stream_chat(&self.model.id, &messages).await?;
        collect_answer(stream, on_delta).await
    }
}

/// Forwards every delta and returns the answer text; reasoning is never part of the answer.
async fn collect_answer(
    mut stream: impl ChatStream,
    on_delta: &mut (impl FnMut(&ChatDelta) + Send),
) -> Result<String> {
    let mut answer = String::new();
    let mut received = 0;
    while let Some(delta) = stream.next_delta().await? {
        let text = match &delta {
            ChatDelta::Reasoning(text) | ChatDelta::Text(text) => text,
        };
        received += text.len();
        ensure!(received <= MAX_ANSWER_BYTES, "model answer is too large");
        if let ChatDelta::Text(text) = &delta {
            answer.push_str(text);
        }
        on_delta(&delta);
    }
    ensure!(!answer.trim().is_empty(), "model returned an empty answer");
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct ScriptedStream(VecDeque<Result<ChatDelta>>);

    impl ChatStream for ScriptedStream {
        async fn next_delta(&mut self) -> Result<Option<ChatDelta>> {
            self.0.pop_front().transpose()
        }
    }

    fn stream(items: impl IntoIterator<Item = Result<ChatDelta>>) -> ScriptedStream {
        ScriptedStream(items.into_iter().collect())
    }

    #[tokio::test]
    async fn forwards_deltas_and_keeps_only_text_as_answer() {
        let mut seen = Vec::new();
        let answer = collect_answer(
            stream([
                Ok(ChatDelta::Reasoning("think".into())),
                Ok(ChatDelta::Text("Hel".into())),
                Ok(ChatDelta::Text("lo".into())),
            ]),
            &mut |delta: &ChatDelta| seen.push(delta.clone()),
        )
        .await
        .unwrap();
        assert_eq!(answer, "Hello");
        assert_eq!(seen.len(), 3);
    }

    #[tokio::test]
    async fn empty_answer_is_an_error() {
        let result = collect_answer(
            stream([Ok(ChatDelta::Reasoning("only thinking".into()))]),
            &mut |_: &ChatDelta| {},
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn oversized_answer_is_an_error() {
        let chunk = "a".repeat(MAX_ANSWER_BYTES / 2 + 1);
        let result = collect_answer(
            stream([
                Ok(ChatDelta::Reasoning(chunk.clone())),
                Ok(ChatDelta::Text(chunk)),
            ]),
            &mut |_: &ChatDelta| {},
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stream_error_stops_collection() {
        let result = collect_answer(
            stream([
                Ok(ChatDelta::Text("partial".into())),
                Err(anyhow::anyhow!("cut")),
            ]),
            &mut |_: &ChatDelta| {},
        )
        .await;
        assert!(result.is_err());
    }
}
