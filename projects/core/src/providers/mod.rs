//! Model providers: one interface for catalogue discovery and streamed chat, with one
//! event type for everything a model sends back.

mod chat;
mod inline_tags;
mod model_output;
mod openai;
mod provider;
mod sse;

pub use chat::{ChatMessage, ChatRequest, ToolSpec};
pub use model_output::{ChatStream, FinishReason, ModelEvent, ModelOutput, ToolCall, Usage};
pub use provider::{Model, ModelProvider, Provider, ProviderRegistry, ProviderStream};
