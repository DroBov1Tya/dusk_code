//! Tools the agent can call: one interface and one registry for every kind of tool.

mod builtin;
mod files;
mod history;
mod load_skill;
mod registry;
mod scope;
mod search;
mod shell;
mod tool;

pub(crate) use builtin::read_regular;
pub(crate) use history::ReadHistory;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::ToolScope;
pub use tool::{Tool, ToolContext};
