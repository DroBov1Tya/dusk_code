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
pub use hello::{Hello, PROTOCOL_VERSION};
pub use request::{
    CatalogRequest, MAX_PICTURE_UPLOAD, MAX_SIM_TEXT, MAX_TERMINAL_INPUT, MAX_TERMINAL_SIDE,
    MAX_TERMINALS, PluginRequest, ProjectRequest, ProviderTarget, Request, SettingsRequest,
    SimulatorRequest, Switch, TerminalRequest, ThreadRequest, VoiceRequest, WorkRequest, wav,
};
pub use response::Response;
pub use secret::Secret;
pub use status::{ChatGpt, Status};
