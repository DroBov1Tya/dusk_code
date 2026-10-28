//! Model providers: catalogue discovery and streamed chat behind one interface.

mod chat;
mod completion_stream;
mod model;
mod openai_compatible;
mod provider;
mod provider_registry;
mod sse_decoder;

pub use chat::{ChatDelta, ChatMessage, ChatRole, ChatStream};
pub use model::{Model, ModelProvider};
pub use provider::{Provider, ProviderStream};
pub use provider_registry::ProviderRegistry;
