//! MCP servers the config lists: local ones the core starts as child processes, remote
//! ones it reaches over HTTP. Each is started in the background when the core starts;
//! its tools join every thread started after it is ready. A server that fails stays
//! failed, and says why, until it is restarted from a client or the core starts again.

mod client;
mod oauth;
mod servers;

pub use client::{McpClient, RemoteTool, check_headers, check_url};
pub use oauth::TokenFile;
pub use servers::{McpInfo, McpServer, McpServers, McpState, McpStatus, McpToolInfo};
