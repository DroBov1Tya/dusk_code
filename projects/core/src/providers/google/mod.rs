//! Google AI (Gemini API): `generateContent`, streamed. Thought summaries are shown and
//! never sent back. The signature a function call comes with must return with that call
//! while its turn lasts; the history has no place for one, so `signatures` remembers
//! them for this run. Signatures on text parts are optional for the API and are dropped.

mod client;
mod request;
mod signatures;
mod stream;

pub use client::Google;
pub use stream::GenerateStream;
