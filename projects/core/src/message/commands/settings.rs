//! `/settings`: everything a client offers to switch, read fresh. The agents folder is
//! read again first, so an agent added a moment ago is in the tree.

use anyhow::Result;

use crate::{
    catalog::agents::AgentTree, catalog::skills::SkillInfo, config::ApprovalPolicy, config::Memory,
    config::WorkHistory, mcp::McpInfo, release::update::Channel, runtime::Runtime, tools::ToolKind,
};

pub struct Settings {
    pub max_agents_at_work: usize,
    pub max_goal_rounds: u32,
    pub idle_screen_minutes: u32,
    pub memory: Memory,
    pub work_history: WorkHistory,
    pub update_channel: Channel,
    pub approvals: ApprovalPolicy,
    /// Commands can run confined on this machine.
    pub sandbox: bool,
    /// Log files and traces; off writes and sends nothing.
    pub logging: bool,
    /// The chat's mode, whose switches `tools` shows.
    pub mode: String,
    /// Every tool by kind and then by name.
    pub tools: Vec<ToolInfo>,
    pub skills: Vec<SkillInfo>,
    pub agents: AgentTree,
    pub mcp: Vec<McpInfo>,
}

pub struct ToolInfo {
    pub name: String,
    pub kind: ToolKind,
    pub enabled: bool,
}

pub async fn run(runtime: &Runtime) -> Result<Settings> {
    runtime.read_agents().await?;
    let config = runtime.config().current();
    let tools = runtime
        .tool_kinds()
        .into_iter()
        .map(|(name, kind)| ToolInfo {
            enabled: config.mode_tool_enabled(config.mode(), &name, kind),
            name,
            kind,
        })
        .collect();
    Ok(Settings {
        max_agents_at_work: config.limits().max_agents_at_work,
        max_goal_rounds: config.limits().max_goal_rounds,
        idle_screen_minutes: config.appearance().idle_screen_minutes,
        mode: config.mode().to_owned(),
        memory: config.memory(),
        work_history: config.work_history(),
        update_channel: config.update_channel(),
        approvals: config.approvals(),
        sandbox: runtime.platform().sandbox,
        logging: config.logging().enabled,
        tools,
        skills: runtime.skills(),
        agents: runtime.agent_tree(),
        mcp: runtime.mcp_servers(),
    })
}
