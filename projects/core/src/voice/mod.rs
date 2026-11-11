//! Speech through an OpenAI-compatible server: a recording goes to `/audio/transcriptions`
//! and comes back as text, a text goes to `/audio/speech` and comes back as sound. The
//! server is whatever the user set up: a hosted service, or a model run on this machine.
//! The key travels as a bearer token and is never logged; a redirect is never followed, so
//! the key stays with the server it was given to.

mod endpoint;
mod local;
mod markdown;
mod multipart;
mod send;
mod speak;
#[cfg(test)]
mod tests;
mod transcribe;

pub use endpoint::check_endpoint;
pub use local::LocalVoice;
pub use speak::speak;
pub use transcribe::transcribe;
