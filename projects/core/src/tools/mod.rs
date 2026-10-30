//! Tools the agent can call: one interface and one registry for every kind of tool.

//!
//! Each kind of tool has a folder: `files`, `console`, `agents` and `harness`, the tools
//! about the thread itself. MCP tools come from servers; `mcp` wraps them.

mod agents;
mod builtin;
mod console;
mod files;
mod harness;
mod mcp;
mod registry;
mod scope;
mod tool;

pub use agents::{AskAgent, MessageParent};
pub use builtin::read_regular;
pub use harness::{AskUser, DisplayImage, Question, ReadHistory, Remember, Tasks};
pub use mcp::McpTool;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::ToolScope;
pub use tool::{Progress, Tool, ToolContext, ToolKind};
