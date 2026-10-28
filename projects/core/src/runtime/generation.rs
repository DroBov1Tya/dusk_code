//! One model call: streams the response to a conversation into a caller-owned `ModelOutput`.

use anyhow::Result;

use crate::providers::{ChatRequest, ChatStream, ModelEvent, ModelOutput, ModelProvider, ToolCall};

/// The output is owned by the caller so what the model wrote survives when the caller
/// stops this future early. Returns the tool calls of a normally finished response.
pub(super) async fn generate<P: ModelProvider>(
    provider: &P,
    request: ChatRequest<'_>,
    output: &mut ModelOutput,
    mut on_event: impl FnMut(&ModelEvent) + Send,
) -> Result<Vec<ToolCall>> {
    let mut stream = provider.stream_chat(request).await?;
    while let Some(event) = stream.next_event().await? {
        output.apply(&event)?;
        on_event(&event);
    }
    output.finish()
}
