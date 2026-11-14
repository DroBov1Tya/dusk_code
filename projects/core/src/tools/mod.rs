//! Tools the agent can call: one interface and one registry for every kind of tool.

//!
//! Each kind of tool has a folder: `files`, `console`, `agents` and `harness`, the tools
//! about the thread itself. MCP tools come from servers; `mcp` wraps them. On macOS,
//! `simulator` is the iOS Simulator.

mod agents;
mod approvals;
mod builtin;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod computer;
pub mod console;
mod files;
mod harness;
pub mod mcp;
mod registry;
mod schema;
mod scope;
#[cfg(target_os = "macos")]
mod simulator;
mod tool;

pub use agents::{AgentState, AskAgent, MessageParent};
pub use approvals::{ApprovalRequest, Approvals, Decision, Judge, Request, Verdict, WindowAccess};
#[cfg(target_os = "macos")]
pub use computer::WindowSession;
#[cfg(target_os = "macos")]
pub use computer::run_overlay_if_asked;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use computer::{Click, PressKeys, Screen, Screenshot, Scroll, TypeText};
pub use console::{Jobs, RunCommand};
pub use harness::{
    AskUser, DisplayAudio, DisplayImage, GoalTool, NameSession, PlanEdit, Question, ReadHistory,
    Remember, Tasks, is_sound, save_sound,
};
pub(crate) use harness::{PLAN_NAME, clear_plan, project_folder, read_plan, save_plan};
pub use mcp::McpTool;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::{Place, ToolScope};
#[cfg(target_os = "macos")]
pub use simulator::IosSimulator;
pub use tool::{Progress, Tool, ToolContext, ToolKind};
