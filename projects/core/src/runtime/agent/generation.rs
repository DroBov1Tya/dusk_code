//! One model call: streams the response to a conversation into a caller-owned `ModelOutput`.

use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::providers::{ChatRequest, ModelEvent, ModelOutput, ModelProvider, ToolCall};

const MAX_ANSWER_TIME: Duration = Duration::from_secs(30 * 60);

/// A call that stayed silent this long is given up on, and the turn may try again.
fn silent(idle: Duration) -> anyhow::Error {
    let message = format!("the model was silent for {} seconds", idle.as_secs());
    crate::providers::ProviderError::new(crate::providers::ProviderErrorKind::TimedOut, message)
        .into()
}

/// The output is owned by the caller so what the model wrote survives when the caller
/// stops this future early. Returns the tool calls of a normally finished response.
/// The provider applies the requested token budget. A byte estimate cannot count its tokens.
#[tracing::instrument(
    name = "model_call",
    skip_all,
    fields(
        model = request.model,
        messages = request.messages.len(),
        tools = request.tools.len(),
        input_tokens = tracing::field::Empty,
        output_tokens = tracing::field::Empty,
    )
)]
pub async fn generate(
    provider: &dyn ModelProvider,
    request: ChatRequest<'_>,
    output: &mut ModelOutput,
    idle: Duration,
    mut on_event: impl FnMut(&ModelEvent) + Send,
) -> Result<Vec<ToolCall>> {
    let streamed = async {
        let mut stream = tokio::time::timeout(idle, provider.stream_chat(request))
            .await
            .map_err(|_| silent(idle))??;
        loop {
            let next = tokio::time::timeout(idle, stream.next_event())
                .await
                .map_err(|_| silent(idle))?;
            let Some(event) = next? else { break };
            output.apply(&event)?;
            on_event(&event);
        }
        Ok::<_, anyhow::Error>(())
    };
    // A server can keep a stream alive with pings and never finish it.
    tokio::time::timeout(MAX_ANSWER_TIME, streamed)
        .await
        .map_err(|_| {
            anyhow!(
                "the model answered for over {} minutes",
                MAX_ANSWER_TIME.as_secs() / 60
            )
        })??;
    if let Some(usage) = output.usage {
        let span = tracing::Span::current();
        span.record("input_tokens", usage.prompt_tokens);
        span.record("output_tokens", usage.completion_tokens);
    }
    output.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ChatStream, FinishReason, Model, is_transient};
    use crate::{BoxFuture, providers::ModelEvent};

    /// A server that accepts the call and then says nothing at all.
    struct Silent;

    struct Never;

    impl ModelProvider for Silent {
        fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn stream_chat<'a>(&'a self, _: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
            Box::pin(async { Ok(Box::new(Never) as Box<dyn ChatStream>) })
        }
    }

    impl ChatStream for Never {
        fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(Some(ModelEvent::Finished(FinishReason::Stop)))
            })
        }
    }

    #[tokio::test]
    async fn a_silent_stream_is_given_up_and_may_be_tried_again() {
        let request = ChatRequest {
            model: "fake",
            messages: &[],
            tools: &[],
            max_output_tokens: None,
            effort: None,
            cache_key: None,
        };
        let mut output = ModelOutput::default();
        let error = generate(
            &Silent,
            request,
            &mut output,
            Duration::from_millis(20),
            |_| {},
        )
        .await
        .expect_err("a model that never answers cannot finish a call");
        assert!(is_transient(&error), "a retry may pass: {error:#}");
        assert!(format!("{error:#}").contains("silent"), "{error:#}");
    }
}
