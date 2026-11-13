//! Model providers: one interface for catalogue discovery and streamed chat, with one
//! event type for everything a model sends back.
//!
//! `registry` builds the configured providers and dispatches to their protocol,
//! `openai`, `anthropic` or `google`. Each sends a `request`, reaches its server through
//! `transport` and turns what comes back into the events of `output`; `errors` names the
//! failures the agent tells apart, `limits` remembers the rate limits an answer reports.

mod anthropic;
pub mod balance;
pub mod cost;
mod errors;
mod google;
mod limits;
pub mod openai;
pub mod output;
mod rate;
mod registry;
mod request;
pub(crate) mod transport;

pub use errors::{
    OutputLimit, RetryAfter, Transient, is_output_limit, is_transient, is_unsupported_effort,
    is_unsupported_pictures, retry_after, stream_error,
};
#[cfg(test)]
pub use output::{ChatStream, FinishReason};
pub use output::{ModelEvent, ModelOutput, ToolCall, Usage, text_calls};
pub use registry::{Model, ModelProvider, Provider, ProviderRegistry, validate_provider};
pub use request::{ChatMessage, ChatRequest, Picture, ToolSpec};
pub use transport::http::is_loopback;
