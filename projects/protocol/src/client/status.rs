//! The status a client draws from.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::{ApprovalPolicy, ContextPolicy, McpStatus, PluginInfo};
use crate::model::Effort;
use crate::release::InstallProgress;
/// The sign-in with a ChatGPT account, which connects OpenAI without an API key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatGpt {
    #[default]
    SignedOut,
    /// The browser has the sign-in page; the core waits for it to come back.
    Waiting,
    SignedIn {
        /// The account's email, or a plain name when it gave none.
        account: String,
    },
    /// The last sign-in did not complete, and why.
    Failed(String),
}

/// Everything a client shows that changes without being asked: sent again whole
/// whenever any of it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The model threads talk to, as `provider/model`.
    pub model: Option<String>,
    /// The core is still looking for a model over the network.
    pub discovering_model: bool,
    pub input_limit: u64,
    pub context_policy: Option<ContextPolicy>,
    pub effort: Option<Effort>,
    pub mode: String,
    pub approvals: ApprovalPolicy,
    pub max_goal_rounds: u32,
    pub idle_screen_minutes: u32,
    /// Whether clients notify at all.
    pub notifications: bool,
    pub sound_notifications: bool,
    pub mcp: Vec<McpStatus>,
    pub mcp_starting: bool,
    pub plugins: Vec<PluginInfo>,
    /// The newer release found on the update channel.
    pub available_update: Option<String>,
    /// How far an update install got, while one runs or once it is done.
    pub install: Option<InstallProgress>,
    /// Shipped files this start brought to the build's version.
    pub updated_builtin: Vec<String>,
    /// How the sign-in with ChatGPT stands.
    #[serde(default)]
    pub chatgpt: ChatGpt,
    /// Where pictures of messages and tool results are kept.
    pub artifacts_dir: PathBuf,
    /// Where a chat without a project works: the root of the workspace folders.
    pub workspace_dir: PathBuf,
}
