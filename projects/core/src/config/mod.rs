//! The settings in `config.yml`: providers, MCP servers, what threads record and which
//! tools and skills are on.
//! `LiveConfig` keeps the current copy in memory and writes changes back.

mod context;
mod effort;
mod file;
pub mod live;
mod mcp;
mod providers;
mod sections;

pub use context::{ContextPolicy, ContextPreset};
pub use effort::Effort;
pub use file::Config;
pub use mcp::{
    HttpServerConfig, McpServerConfig, StdioServerConfig, mcp_server_yaml, parse_mcp_server,
};
pub use providers::{ApiKey, ProviderConfig, ProviderKind};
pub use sections::{
    Appearance, ApprovalPolicy, Limits, Logging, Memory, RecordMode, ThreadConfig, Traces,
    WorkHistory,
};
