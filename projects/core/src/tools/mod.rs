//! Tools the agent can call: one interface and one registry for every kind of tool.

//!
//! Each kind of tool has a folder: `files`, `console`, `agents` and `harness`, the tools
//! about the thread itself. MCP tools come from servers; `mcp` wraps them.

mod agents;
mod approvals;
mod builtin;
#[cfg(target_os = "macos")]
mod computer;
mod console;
mod files;
mod harness;
pub mod mcp;
mod registry;
mod schema;
mod scope;
mod tool;

pub use agents::{AgentState, AskAgent, MessageParent};
pub use approvals::{ApprovalRequest, Approvals, Decision, Judge, Request, Verdict};
#[cfg(target_os = "macos")]
pub use computer::{Click, PressKeys, Screen, Screenshot, Scroll, TypeText, run_overlay_if_asked};
pub use console::JobInfo;
pub(crate) use console::{JobManager, JobOwner, Jobs, RunCommand};
pub use harness::{AskUser, DisplayImage, GoalTool, Question, ReadHistory, Remember, Tasks};
pub use mcp::McpTool;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::{Place, ToolScope};
pub use tool::{Progress, Tool, ToolContext, ToolKind};
