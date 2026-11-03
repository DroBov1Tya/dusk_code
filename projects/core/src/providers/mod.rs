//! Model providers: one interface for catalogue discovery and streamed chat, with one
//! event type for everything a model sends back.

mod anthropic;
mod chat;
mod http;
mod model_output;
mod openai;
mod provider;
mod sse;
pub mod text_calls;

pub use sse::SseDecoder;

pub use chat::{ChatMessage, ChatRequest, Picture, ToolSpec};
pub use http::is_loopback;
pub use model_output::{
    ChatStream, FinishReason, ModelEvent, ModelOutput, OutputLimit, ToolCall, Usage,
    is_output_limit,
};
pub use provider::{
    Model, ModelProvider, Provider, ProviderRegistry, Transient, UnsupportedEffort, is_transient,
    is_unsupported_effort, is_unsupported_pictures, validate_provider,
};
