//! The plain types of the settings panel: its categories, rows, switches and hits.

use dusk_protocol::config::{ApprovalPolicy, McpInfo, McpToolInfo};

/// A row of the MCP list: a server, or one of its tools under it.
#[derive(Clone, Copy)]
pub enum McpRow<'a> {
    Server(&'a McpInfo),
    Tool(&'a McpInfo, &'a McpToolInfo),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingCategory {
    Models,
    Agents,
    Tools,
    Skills,
    Mcp,
    /// What the agent keeps about the user.
    Personalize,
    Limits,
    /// Whether tools ask before leaving their folders and before commands.
    Access,
    /// The prefix of the branch names the tool creates.
    VersionControl,
    /// How the app looks and behaves on screen.
    Appearance,
    Updates,
    System,
}

impl SettingCategory {
    pub const ALL: [Self; 12] = [
        Self::Models,
        Self::Agents,
        Self::Skills,
        Self::Mcp,
        Self::Tools,
        Self::Personalize,
        Self::Limits,
        Self::Access,
        Self::VersionControl,
        Self::Appearance,
        Self::Updates,
        Self::System,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Agents => "agents",
            Self::Tools => "tools",
            Self::Skills => "skills",
            Self::Mcp => "mcp",
            Self::Personalize => "personalize",
            Self::Limits => "limits",
            Self::Access => "access",
            Self::VersionControl => "version control",
            Self::Appearance => "appearance",
            Self::Updates => "updates",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwitchKind {
    Agent,
    Tool,
    Skill,
    Mcp,
    Memory,
    WorkHistory,
    /// On is the dev channel, off the stable one.
    DevChannel,
    /// On sets this approval policy, off goes back to asking.
    Approvals(ApprovalPolicy),
    /// Off writes no logs and sends no traces.
    Logging,
}

/// The policies the access rows switch on, in their order; logs come after.
pub const ACCESS_POLICIES: [ApprovalPolicy; 3] = [
    ApprovalPolicy::FullAccess,
    ApprovalPolicy::Sandbox,
    ApprovalPolicy::Arbiter,
];

/// One row of the agent tree, with what is needed to draw the traces to it.
pub struct AgentRow {
    pub name: String,
    pub description: String,
    pub version: Option<i64>,
    pub enabled: bool,
    /// The agents it may ask.
    pub links: Vec<String>,
    /// On, and every agent above it too: only then can it be asked.
    pub active: bool,
    pub depth: usize,
    /// For each level above this row: whether the trace goes on below it.
    pub rails: Vec<bool>,
    /// The last of its siblings.
    pub last: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum EditTarget {
    Skill(String),
    /// An agent's AGENT.md, by the agent's name.
    Agent(String),
    /// `None` for a server not saved yet.
    Mcp(Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileButton {
    Save,
    Revert,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    /// A row of the list, by its index.
    Row(usize),
    /// The switch on a row.
    Switch(usize),
    Button(FileButton),
    MemorySize {
        row: usize,
        more: bool,
    },
}

pub struct SettingsHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub target: HitTarget,
}
