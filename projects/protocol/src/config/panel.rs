//! The settings window: limits, policies and every tool, skill, agent and server.

use serde::{Deserialize, Serialize};

use crate::catalog::{AgentTree, SkillInfo};
use crate::config::{ApprovalPolicy, McpInfo, Memory, VersionControl, WorkHistory};
use crate::release::Channel;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub max_agents_at_work: usize,
    pub max_goal_rounds: u32,
    pub idle_screen_minutes: u32,
    pub memory: Memory,
    pub work_history: WorkHistory,
    pub update_channel: Channel,
    pub approvals: ApprovalPolicy,
    /// Version control settings, shown as their own section.
    pub version_control: VersionControl,
    /// Whether clients notify at all; the bell is the TUI's notification.
    pub notifications: bool,
    /// Whether notifications ring the terminal bell; off stays silent.
    pub sound_notifications: bool,
    /// Commands can run confined on this machine.
    pub sandbox: bool,
    /// Log files and traces; off writes and sends nothing.
    pub logging: bool,
    /// The chat's mode, whose switches `tools` shows.
    pub mode: String,
    /// Every mode, in the order Tab walks them.
    pub modes: Vec<ModeInfo>,
    /// Every tool by kind and then by name.
    pub tools: Vec<ToolInfo>,
    pub skills: Vec<SkillInfo>,
    pub agents: AgentTree,
    pub mcp: Vec<McpInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeInfo {
    pub name: String,
    /// The first sentence of its prompt.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub kind: ToolKind,
    pub enabled: bool,
    /// What the model is told the tool does.
    pub description: String,
}

/// What a tool works on, for clients to group tools by; in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ToolKind {
    Files,
    Console,
    Agents,
    /// The thread itself: its history, tasks and skills.
    Harness,
    Mcp,
    /// The screen: screenshots, mouse and keyboard. Heavy, so off until switched on.
    Computer,
}

impl ToolKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Console => "console",
            Self::Agents => "agents",
            Self::Harness => "harness",
            Self::Mcp => "mcp",
            Self::Computer => "computer",
        }
    }

    /// Whether a tool of this kind is on before the user switches it.
    pub fn on_by_default(self) -> bool {
        self != Self::Computer
    }
}
