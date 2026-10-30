//! Tools the agent can call: one interface and one registry for every kind of tool.

mod ask_user;
mod builtin;
mod files;
mod history;
mod load_skill;
mod mcp;
mod registry;
mod remember;
mod scope;
mod shell;
mod subagents;
mod tasks;
mod tool;

pub use ask_user::{AskUser, Question};
pub use builtin::read_regular;
pub use history::ReadHistory;
pub use mcp::McpTool;
pub use registry::{ToolRegistry, ToolResult};
pub use remember::Remember;
pub use scope::ToolScope;
pub use subagents::{AskAgent, MessageParent};
pub use tasks::Tasks;
pub use tool::{Progress, Tool, ToolContext, ToolKind};
