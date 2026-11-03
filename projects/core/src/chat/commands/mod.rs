//! Commands a client sends as a message: a known `/word` at the very start, optionally
//! followed by an argument. Anything else is an ordinary message, `/etc/hosts` included.
//!
//! Each command has a file of its own with what it does. Thread commands run in the
//! thread, in order with its messages; client commands answer the client at once, which
//! only shows the answer.

mod command;
mod compact;
mod context;
mod goal;
mod model;
mod resume;
mod settings;
mod undo;

pub use command::{Answer, COMMANDS, Command, parse};
pub use context::{ContextReport, ContextSection, report as context_report};
pub use settings::{Settings, ToolInfo};
