//! Tools the agent can call: one interface and one registry for every kind of tool.

mod builtin;
mod files;
mod registry;
mod scope;
mod search;
mod shell;
mod tool;

pub use registry::{ToolRegistry, ToolResult};
pub use scope::ToolScope;
pub use tool::{Tool, ToolFuture};
