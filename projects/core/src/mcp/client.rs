//! One MCP server, reached with the official Rust SDK: a local server the core starts as
//! a child process over stdio, or a remote one over Streamable HTTP. What the SDK leaves
//! open is set here: the child's stderr is dropped, since the client may be drawing on
//! the terminal; remote URLs are HTTPS or this machine; redirects are not followed; the
//! configured headers are secret; every request has a time limit.

use std::{collections::HashMap, process::Stdio, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{
    Client, Url,
    header::{HeaderName, HeaderValue},
    redirect::Policy,
};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ContentBlock},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::Value;
use tokio::process::Command;

use crate::{
    config::{HttpServerConfig, McpServerConfig, StdioServerConfig},
    providers::is_loopback,
};

const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct McpClient {
    service: RunningService<RoleClient, ()>,
}

/// A tool as the server describes it.
#[derive(Debug, Clone)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// HTTPS, or plain HTTP to this machine by its address, without credentials in the URL.
pub fn check_url(url: &str) -> Result<Url> {
    let url = Url::parse(url).map_err(|_| anyhow!("invalid MCP server URL"))?;
    ensure!(
        url.scheme() == "https" && url.host_str().is_some() || is_loopback(&url),
        "MCP server URL must use HTTPS, or HTTP to 127.0.0.1 or [::1]"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "MCP server URL must not contain credentials or a fragment"
    );
    Ok(url)
}

/// The config's headers as they go out, every value marked sensitive.
pub fn check_headers(config: &HttpServerConfig) -> Result<HashMap<HeaderName, HeaderValue>> {
    let mut headers = HashMap::new();
    for (name, value) in &config.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| anyhow!("invalid MCP header name {name:?}"))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| anyhow!("invalid value of MCP header {name}"))?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    Ok(headers)
}

impl McpClient {
    /// Starts or reaches the server and agrees on the protocol with it.
    pub async fn start(config: &McpServerConfig) -> Result<Self> {
        let service = match config {
            McpServerConfig::Stdio(config) => start_child(config).await?,
            McpServerConfig::Http(config) => reach(config).await?,
        };
        Ok(Self { service })
    }

    pub async fn list_tools(&self) -> Result<Vec<RemoteTool>> {
        let tools = tokio::time::timeout(START_TIMEOUT, self.service.list_all_tools())
            .await
            .context("the server did not list its tools in time")?
            .map_err(|error| anyhow!("cannot list the tools: {error}"))?;
        Ok(tools
            .into_iter()
            .map(|tool| RemoteTool {
                name: tool.name.into_owned(),
                description: tool
                    .description
                    .map(|text| text.into_owned())
                    .unwrap_or_default(),
                input_schema: Value::Object((*tool.input_schema).clone()),
            })
            .collect())
    }

    /// The text of the result; a result the server marks as an error is an error.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let Value::Object(arguments) = arguments else {
            bail!("arguments must be a JSON object");
        };
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let result = tokio::time::timeout(CALL_TIMEOUT, self.service.call_tool(params))
            .await
            .map_err(|_| anyhow!("the server did not answer in {}s", CALL_TIMEOUT.as_secs()))?
            .map_err(|error| anyhow!("the call failed: {error}"))?;
        let mut text: Vec<String> = result.content.iter().map(content_text).collect();
        if text.is_empty()
            && let Some(structured) = &result.structured_content
        {
            text.push(structured.to_string());
        }
        let text = text.join("\n");
        if result.is_error == Some(true) {
            bail!("{text}");
        }
        Ok(text)
    }
}

async fn start_child(config: &StdioServerConfig) -> Result<RunningService<RoleClient, ()>> {
    let mut command = Command::new(&config.command);
    command.args(&config.args).envs(&config.env);
    let (transport, _) = TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("cannot start {}", config.command))?;
    tokio::time::timeout(START_TIMEOUT, ().serve(transport))
        .await
        .context("the server did not start in time")?
        .map_err(|error| anyhow!("initialize: {error}"))
}

async fn reach(config: &HttpServerConfig) -> Result<RunningService<RoleClient, ()>> {
    let url = check_url(&config.url)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| anyhow!("cannot build MCP HTTP client"))?;
    let transport_config = StreamableHttpClientTransportConfig::with_uri(url.as_str())
        .custom_headers(check_headers(config)?);
    let transport = StreamableHttpClientTransport::with_client(client, transport_config);
    // The error text may quote the URL, which may carry a key in its query.
    tokio::time::timeout(START_TIMEOUT, ().serve(transport))
        .await
        .context("the server did not answer in time")?
        .map_err(|error| {
            anyhow!(
                "initialize: {}",
                error.to_string().replace(url.as_str(), "<url>")
            )
        })
}

/// Text parts as they are; other parts are named, since the model reads only text.
fn content_text(part: &ContentBlock) -> String {
    match part {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(_) => "[image content]".into(),
        ContentBlock::Audio(_) => "[audio content]".into(),
        other => match serde_json::to_value(other) {
            Ok(value) => format!(
                "[{} content]",
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
            Err(_) => "[unknown content]".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn non_text_parts_are_named() {
        let text: ContentBlock =
            serde_json::from_value(json!({ "type": "text", "text": "two files" })).unwrap();
        let image: ContentBlock = serde_json::from_value(
            json!({ "type": "image", "data": "AAAA", "mimeType": "image/png" }),
        )
        .unwrap();
        assert_eq!(content_text(&text), "two files");
        assert_eq!(content_text(&image), "[image content]");
    }
}
