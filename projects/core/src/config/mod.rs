//! The settings in `config.yml`: providers, MCP servers, plugins, what threads record
//! and which tools and skills are on.
//! `LiveConfig` keeps the current copy in memory and writes changes back.

mod file;
pub mod live;
mod mcp;
mod plugins;
mod providers;
mod sections;

pub use dusk_protocol::config::{ContextPolicy, UNKNOWN_WINDOW, VersionControl};
pub use dusk_protocol::model::Effort;
pub use file::Config;
pub use mcp::{
    HttpServerConfig, McpServerConfig, StdioServerConfig, mcp_server_yaml, parse_mcp_server,
};
pub use plugins::{Plugged, PluginConfig};
pub use providers::{ApiKey, BalanceApi, Price, ProviderConfig, ProviderKind};
pub use sections::{
    Appearance, ApprovalPolicy, DEFAULT_MODEL_TIMEOUT_SECONDS, Limits, Logging, MAX_ATTEMPTS,
    MAX_MODEL_TIMEOUT_SECONDS, MIN_ATTEMPTS, MIN_MODEL_TIMEOUT_SECONDS, Memory, ThreadConfig,
    Traces, WorkHistory,
};
