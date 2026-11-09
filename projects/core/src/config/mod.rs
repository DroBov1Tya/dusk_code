//! The settings in `config.yml`: providers, MCP servers, plugins, what threads record
//! and which tools and skills are on.
//! `LiveConfig` keeps the current copy in memory and writes changes back.

mod context;
mod effort;
mod file;
pub mod live;
mod mcp;
mod plugins;
mod providers;
mod sections;

pub use context::ContextPolicy;
pub use dusk_protocol::config::{UNKNOWN_WINDOW, VersionControl};
pub use effort::Effort;
pub use file::Config;
pub use mcp::{
    HttpServerConfig, McpServerConfig, StdioServerConfig, mcp_server_yaml, parse_mcp_server,
};
pub use plugins::{Plugged, PluginConfig};
pub use providers::{ApiKey, ProviderConfig, ProviderKind};
pub use sections::{
    Appearance, ApprovalPolicy, Limits, Logging, Memory, ThreadConfig, Traces, WorkHistory,
};
