//! `/settings`: everything a client offers to switch, read fresh. The agents folder is
//! read again first, so an agent added a moment ago is in the tree.

use anyhow::Result;

use crate::{
    agents::AgentTree, mcp::McpInfo, runtime::Runtime, skills::SkillInfo, tools::ToolKind,
};

pub struct Settings {
    pub max_agents_at_work: usize,
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
            enabled: config.tool_enabled(&name),
            name,
            kind,
        })
        .collect();
    Ok(Settings {
        max_agents_at_work: config.limits().max_agents_at_work,
        tools,
        skills: runtime.skills(),
        agents: runtime.agent_tree(),
        mcp: runtime.mcp_servers(),
    })
}
