//! The files the user edits through the settings, and the MCP servers.

use serde::{Deserialize, Serialize};

use crate::config::Profile;
/// The files the user edits through the settings, and the MCP servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogRequest {
    Skill {
        name: String,
    },
    SaveSkill {
        name: String,
        text: String,
    },
    Agent {
        name: String,
    },
    SaveAgent {
        name: String,
        text: String,
    },
    McpServers,
    McpServerText {
        name: String,
    },
    /// Answers with the name it was saved under, or none when the text removed it.
    SaveMcpServer {
        old: Option<String>,
        text: String,
    },
    RestartMcp {
        name: String,
    },
    /// Answers with the page that signs in.
    SignInMcp {
        name: String,
    },
    Snippets,
    /// Empty text removes the snippet.
    SaveSnippet {
        name: String,
        text: String,
    },
    Profile,
    SaveProfile(Profile),
    /// Which secrets are set, without their values.
    Secrets,
    /// The speech servers that are set up.
    Voice,
}
