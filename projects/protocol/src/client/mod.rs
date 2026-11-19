//! How a client talks to the core: the hello, the requests it makes, the responses, the events
//! and the status it draws from, and the slash commands it sends as messages.

mod command;
mod event;
mod frame;
mod hello;
mod request;
mod response;
mod secret;
mod status;

pub use command::{Answer, COMMANDS, Command, parse};
pub use event::Event;
pub use frame::{RequestFrame, ServerFrame};
pub use hello::{
    Busy, Claim, CoreInfo, Greeting, Hello, OLDEST_PROTOCOL, OwnerKind, PROTOCOL_VERSION, Proof,
    Refusal, Then, client_proof, core_proof, nonce, nonce_is_valid, proof_matches, token_key,
    try_nonce,
};
pub use request::{
    CatalogRequest, GitRequest, MAX_PICTURE_UPLOAD, MAX_TERMINAL_INPUT, MAX_TERMINAL_SIDE,
    MAX_TERMINALS, PluginRequest, ProjectRequest, ProviderTarget, Request, SettingsRequest, Switch,
    TerminalRequest, ThreadRequest, VoiceRequest, WorkRequest, wav,
};
pub use response::Response;
pub use secret::Secret;
pub use status::{CHATGPT_USAGE_URL, ChatGpt, Status};
