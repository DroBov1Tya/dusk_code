//! Tools the agent can call: one interface and one registry for every kind of tool.

mod builtin;
mod files;
mod history;
mod load_skill;
mod registry;
mod scope;
mod shell;
mod tool;

pub use builtin::read_regular;
pub use history::ReadHistory;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::ToolScope;
pub use tool::{Progress, Tool, ToolContext};
