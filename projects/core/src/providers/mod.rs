//! Model providers: catalogue discovery and streamed chat behind one interface.

mod chat;
mod openai;
mod provider;
mod sse;

pub use chat::{ChatDelta, ChatMessage, ChatRole, ChatStream};
pub use provider::{Model, ModelProvider, Provider, ProviderRegistry, ProviderStream};
