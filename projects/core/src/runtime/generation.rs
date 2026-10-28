//! One model call: streams the response to a conversation into a caller-owned `ModelOutput`.

use anyhow::Result;

use crate::providers::{
    ChatRequest, ModelEvent, ModelOutput, ModelProvider, OutputLimit, ToolCall,
};

/// The output is owned by the caller so what the model wrote survives when the caller
/// stops this future early. Returns the tool calls of a normally finished response.
///
/// Past `max_output_tokens` by its own count the stream is dropped, which closes the
/// connection, in case the provider ignores the limit it was sent.
pub(super) async fn generate(
    provider: &dyn ModelProvider,
    request: ChatRequest<'_>,
    output: &mut ModelOutput,
    mut on_event: impl FnMut(&ModelEvent) + Send,
) -> Result<Vec<ToolCall>> {
    let mut stream = provider.stream_chat(request).await?;
    while let Some(event) = stream.next_event().await? {
        output.apply(&event)?;
        on_event(&event);
        if output.estimated_tokens() > request.max_output_tokens {
            return Err(OutputLimit.into());
        }
    }
    output.finish()
}
