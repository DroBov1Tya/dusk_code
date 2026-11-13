//! OpenAI-compatible Chat Completions API (OpenAI, LiteLLM, vLLM and others), and the
//! Responses API on a ChatGPT plan with the sign-in that opens it.

mod client;
mod request;
mod responses;
pub mod sign_in;
mod stream;

pub use client::OpenAiCompatible;
pub use responses::{ChatGpt, ResponseStream};
pub use stream::CompletionStream;
