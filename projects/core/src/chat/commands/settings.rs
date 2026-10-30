//! `/settings`: everything a client offers to switch, read fresh. The agents folder is
//! read again first, so an agent added a moment ago is in the tree.

use anyhow::Result;

use crate::{
    catalog::agents::AgentTree, catalog::skills::SkillInfo, config::Memory, mcp::McpInfo,
    release::update::Channel, runtime::Runtime, tools::ToolKind,
};

pub struct Settings {
    pub max_agents_at_work: usize,
    pub memory: Memory,
    pub update_channel: Channel,
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
        mode: config.mode().to_owned(),
        memory: config.memory(),
        update_channel: config.update_channel(),
        tools,
        skills: runtime.skills(),
        agents: runtime.agent_tree(),
        mcp: runtime.mcp_servers(),
    })
}
