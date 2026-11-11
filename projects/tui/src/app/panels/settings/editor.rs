//! The editor of a skill, an agent or an MCP server.

use super::types::{AgentRow, EditTarget};
use crate::app::Input;
use dusk_protocol::{
    catalog::{AgentNode, NodeText},
    config::{McpInfo, McpState},
};

pub struct FileEditor {
    pub target: EditTarget,
    pub metadata: Vec<(String, String)>,
    pub text: Input,
    pub(super) original: String,
    pub scroll: usize,
    /// Why the last save was refused.
    pub error: Option<String>,
}

/// What a new MCP server starts as.
pub(super) const MCP_TEMPLATE: &str = "\
# A local server the core starts. For a remote one, write
#   url: https://host/mcp
#   headers: {Authorization: Bearer ...}
# in place of command, args and env.
name:
  command: npx
  args: [-y, some-mcp-server]
  env: {}
";

impl FileEditor {
    pub fn skill(skill: NodeText) -> Self {
        Self::new(EditTarget::Skill(skill.name), skill.metadata, skill.text)
    }

    pub fn agent(agent: NodeText) -> Self {
        Self::new(EditTarget::Agent(agent.name), agent.metadata, agent.text)
    }

    /// An MCP server's YAML, or the template for a new one, with how it stands.
    pub fn mcp(server: Option<&McpInfo>, text: Option<String>) -> Self {
        let target = EditTarget::Mcp(server.map(|server| server.name.clone()));
        let text = text.unwrap_or_else(|| MCP_TEMPLATE.to_owned());
        Self::new(target, mcp_fields(server), text)
    }

    /// Shows how the open MCP server stands now; nothing for anything else.
    pub fn update_mcp(&mut self, servers: &[McpInfo]) {
        if let EditTarget::Mcp(Some(name)) = &self.target {
            let server = servers.iter().find(|server| server.name == *name);
            self.metadata = mcp_fields(server);
        }
    }

    pub(super) fn new(target: EditTarget, metadata: Vec<(String, String)>, text: String) -> Self {
        Self {
            target,
            metadata,
            text: Input {
                command_cycle: None,
                text: text.clone(),
                cursor: 0,
                pastes: Vec::new(),
                anchor: None,
            },
            original: text,
            scroll: 0,
            error: None,
        }
    }

    pub fn title(&self) -> &str {
        match &self.target {
            EditTarget::Skill(name) | EditTarget::Agent(name) | EditTarget::Mcp(Some(name)) => name,
            EditTarget::Mcp(None) => "new MCP server",
        }
    }

    pub fn changed(&self) -> bool {
        self.text.text != self.original
    }
}

pub(super) fn mcp_fields(server: Option<&McpInfo>) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(server) = server {
        let state = match &server.state {
            McpState::Off => "off".to_owned(),
            McpState::Starting => "starting".to_owned(),
            McpState::Ready(tools) => format!("ready, {} tools", tools.len()),
            McpState::Failed(error) => format!("failed: {error}"),
        };
        fields.push(("state".to_owned(), state));
        if let McpState::Ready(tools) = &server.state {
            fields.push(("tools".to_owned(), tools.join(", ")));
        }
    }
    fields.push((
        "help".to_owned(),
        "save starts it again; saving empty text removes it".to_owned(),
    ));
    fields
}

/// The tree as rows, depth first, each knowing the traces of the levels above it.
pub(super) fn flatten(
    node: &AgentNode,
    depth: usize,
    rails: &[bool],
    last: bool,
    parent_active: bool,
    rows: &mut Vec<AgentRow>,
) {
    let active = parent_active && node.enabled;
    rows.push(AgentRow {
        name: node.name.clone(),
        description: node.description.clone(),
        version: node.version,
        enabled: node.enabled,
        links: node.links.clone(),
        active,
        depth,
        rails: rails.to_vec(),
        last,
    });
    let mut below = rails.to_vec();
    if depth > 0 {
        below.push(!last);
    }
    for (index, child) in node.children.iter().enumerate() {
        let last = index + 1 == node.children.len();
        flatten(child, depth + 1, &below, last, active, rows);
    }
}
