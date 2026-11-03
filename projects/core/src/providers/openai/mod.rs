//! OpenAI-compatible Chat Completions API (OpenAI, LiteLLM, vLLM and others).

mod client;
mod request;
mod stream;

pub use client::OpenAiCompatible;
pub use stream::CompletionStream;
