//! Tools the agent can call: one interface and one registry for every kind of tool.

//!
//! Each kind of tool has a folder: `files`, `console`, `git`, `agents` and `harness`, the
//! tools about the thread itself. MCP tools come from servers; `mcp` wraps them.

mod agents;
mod approvals;
mod builtin;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod computer;
pub mod console;
mod files;
mod git;
mod harness;
pub mod mcp;
mod registry;
mod schema;
mod scope;
mod tool;

pub use agents::{AgentState, AskAgent, MessageParent};
pub use approvals::{ApprovalRequest, Approvals, Decision, Judge, Request, Verdict, WindowAccess};
#[cfg(target_os = "macos")]
pub use computer::WindowSession;
#[cfg(target_os = "macos")]
pub use computer::run_overlay_if_asked;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use computer::{
    Click, ComputerActions, PhysicalInput, PressKeys, Screen, Screenshot, TypeText,
};
pub use console::{Jobs, RunCommand, Shaped};
pub use git::Branch;
pub(crate) use git::{scope_for as worktree_scope, start as start_branch};
pub(crate) use harness::propose_session;
pub use harness::{
    AddWorkshopSource, AskUser, DisplayAudio, DisplayImage, DrawDiagram, GatesYes, PlanEdit,
    ProposeSession, Question, ReadHistory, Remember, SearchWorkshop, Show, Started,
    SuggestFromWorkshop, SuggestNext, Tasks, TeamRunner, is_sound, save_sound,
};
pub(crate) use harness::{
    clear_plan, current_plan_name, list_plans, project_folder, read_plan, remove_old_plans,
    save_plan,
};
pub use mcp::McpTool;
pub use registry::{ToolRegistry, ToolResult};
pub use scope::{Place, ToolScope};
#[cfg(test)]
pub(crate) use tool::UNATTENDED;
pub use tool::{Progress, Tool, ToolContext, ToolKind};
