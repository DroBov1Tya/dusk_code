//! The MCP protocol as the core speaks it, over either transport: agree on a version,
//! list the tools, call one.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{http::HttpTransport, stdio::StdioTransport};
use crate::config::McpServerConfig;

const PROTOCOL_VERSION: &str = "2025-06-18";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Pages of tools a server may list, so a server cannot keep the start going forever.
const MAX_TOOL_PAGES: usize = 20;

enum Transport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

pub struct McpClient {
    transport: Transport,
    next_id: AtomicU64,
}

/// A tool as the server describes it.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema", default = "empty_schema")]
    pub input_schema: Value,
}

impl McpClient {
    /// Starts or reaches the server and agrees on the protocol with it.
    pub async fn start(config: &McpServerConfig) -> Result<Self> {
        let transport = match config {
            McpServerConfig::Stdio(config) => Transport::Stdio(StdioTransport::start(config)?),
            McpServerConfig::Http(config) => Transport::Http(HttpTransport::new(config)?),
        };
        let client = Self {
            transport,
            next_id: AtomicU64::new(1),
        };
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "dusk", "version": env!("CARGO_PKG_VERSION") },
        });
        client
            .request("initialize", params, START_TIMEOUT)
            .await
            .context("initialize")?;
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        match &client.transport {
            Transport::Stdio(stdio) => stdio.send(&initialized).await?,
            Transport::Http(http) => http.send(&initialized).await?,
        }
        Ok(client)
    }

    pub async fn list_tools(&self) -> Result<Vec<RemoteTool>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, START_TIMEOUT).await?;
            let listed: Vec<RemoteTool> =
                serde_json::from_value(page.get("tools").cloned().unwrap_or(json!([])))
                    .context("the server listed tools in an unknown shape")?;
            tools.extend(listed);
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
        bail!("the server lists more than {MAX_TOOL_PAGES} pages of tools")
    }

    /// The text of the result; a result the server marks as an error is an error.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let params = json!({ "name": name, "arguments": arguments });
        let result = self.request("tools/call", params, CALL_TIMEOUT).await?;
        let text = result_text(&result);
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            bail!("{text}");
        }
        Ok(text)
    }

    async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let exchange = async {
            match &self.transport {
                Transport::Stdio(stdio) => stdio.request(id, &message).await,
                Transport::Http(http) => http.request(id, &message).await,
            }
        };
        match tokio::time::timeout(timeout, exchange).await {
            Ok(answer) => result_of(answer?),
            Err(_) => bail!(
                "the server did not answer {method} in {}s",
                timeout.as_secs()
            ),
        }
    }
}

fn result_of(message: Value) -> Result<Value> {
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(anyhow!("the server answered with an error: {text}"));
    }
    message
        .get("result")
        .cloned()
        .context("the server answered without a result")
}

/// Text parts as they are; other parts are named, since the model reads only text.
fn result_text(result: &Value) -> String {
    let parts = result
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut text: Vec<String> = parts
        .iter()
        .map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => part
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Some(kind) => format!("[{kind} content]"),
            None => "[unknown content]".to_owned(),
        })
        .collect();
    if text.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        text.push(structured.to_string());
    }
    text.join("\n")
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_become_text() {
        let result = json!({ "content": [
            { "type": "text", "text": "two files" },
            { "type": "image", "data": "...", "mimeType": "image/png" },
        ]});
        assert_eq!(result_text(&result), "two files\n[image content]");
        let error = json!({ "id": 1, "error": { "code": -32601, "message": "no such method" } });
        assert!(result_of(error).is_err());
    }
}
