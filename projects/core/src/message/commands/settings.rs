//! `/settings`: everything a client offers to switch, read fresh. The agents folder is
//! read again first, so an agent added a moment ago is in the tree.

use anyhow::Result;

use crate::runtime::Runtime;

pub use dusk_protocol::settings::{Settings, ToolInfo};

pub async fn run(runtime: &Runtime) -> Result<Settings> {
    runtime.read_agents().await?;
    let config = runtime.config().current();
    let tools = runtime
        .tool_kinds()?
        .into_iter()
        .map(|(name, kind, description)| ToolInfo {
            enabled: config.mode_tool_enabled(config.mode(), &name, kind),
            name,
            kind,
            description,
        })
        .collect();
    Ok(Settings {
        max_agents_at_work: config.limits().max_agents_at_work,
        max_goal_rounds: config.limits().max_goal_rounds,
        idle_screen_minutes: config.appearance().idle_screen_minutes,
        mode: config.mode().to_owned(),
        modes: runtime.mode_infos(),
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
