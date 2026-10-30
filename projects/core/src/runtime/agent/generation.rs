//! One model call: streams the response to a conversation into a caller-owned `ModelOutput`.

use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::providers::{
    ChatRequest, ModelEvent, ModelOutput, ModelProvider, OutputLimit, ToolCall,
};

const MAX_ANSWER_TIME: Duration = Duration::from_secs(30 * 60);

/// The output is owned by the caller so what the model wrote survives when the caller
/// stops this future early. Returns the tool calls of a normally finished response.
///
/// Past `max_output_tokens` by its own count the stream is dropped, which closes the
/// connection, in case the provider ignores the limit it was sent.
pub async fn generate(
    provider: &dyn ModelProvider,
    request: ChatRequest<'_>,
    output: &mut ModelOutput,
    mut on_event: impl FnMut(&ModelEvent) + Send,
) -> Result<Vec<ToolCall>> {
    let streamed = async {
        let mut stream = provider.stream_chat(request).await?;
        while let Some(event) = stream.next_event().await? {
            output.apply(&event)?;
            on_event(&event);
            if output.estimated_tokens() > request.max_output_tokens {
                return Err(OutputLimit.into());
            }
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
    output.finish()
}
