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
        streamable_http_client::{StreamableHttpClient, StreamableHttpClientTransportConfig},
    },
};
use serde_json::Value;
use tokio::process::Command;

use super::oauth::{self, TokenFile};
use crate::{
    config::{HttpServerConfig, McpServerConfig, StdioServerConfig},
    providers::is_loopback,
};

const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Tool descriptions go into every request, so each one is bounded.
const MAX_DESCRIPTION_CHARS: usize = 1024;
/// More than the model is shown of a result; the registry cuts it further.
const MAX_RESULT_BYTES: usize = 1024 * 1024;
/// Pictures kept from one result; a server sends one screen, not an album.
const MAX_IMAGES: usize = 4;

/// What a call gave: its text, and the pictures in it as bytes, still to be checked.
pub struct CallOutput {
    pub text: String,
    pub images: Vec<Vec<u8>>,
}

pub struct McpClient {
    service: RunningService<RoleClient, ()>,
}

/// A tool as the server describes it.
#[derive(Debug, Clone)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// The server says the tool only reads or is idempotent. The hint comes from the
    /// server and is not trusted for anything beyond repeating that server's own call.
    pub repeatable: bool,
    /// The server says the tool changes nothing. Only `readOnlyHint` says so; an
    /// idempotent call may still change state.
    pub read_only: bool,
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
    /// Starts or reaches the server and agrees on the protocol with it. A remote server
    /// signed in to gets the token from `tokens`.
    pub async fn start(config: &McpServerConfig, tokens: Option<TokenFile>) -> Result<Self> {
        let service = match config {
            McpServerConfig::Stdio(config) => start_child(config).await?,
            McpServerConfig::Http(config) => reach(config, tokens).await?,
        };
        Ok(Self { service })
    }

    pub async fn list_tools(&self) -> Result<Vec<RemoteTool>> {
        let tools = tokio::time::timeout(START_TIMEOUT, self.service.list_all_tools())
            .await
            .context("the server did not list its tools in time")?
            .map_err(|error| anyhow!("cannot list the tools: {error}"))?;
        Ok(tools.into_iter().map(remote_tool).collect())
    }

    /// The connection ended: a local server exited, or a remote one closed it.
    pub fn is_closed(&self) -> bool {
        self.service.is_closed()
    }

    /// The instructions the server gave at the handshake, if any.
    pub fn instructions(&self) -> Option<String> {
        self.service
            .peer_info()
            .and_then(|info| info.instructions.clone())
    }

    /// The text and pictures of the result; a result the server marks as an error is an
    /// error.
    #[tracing::instrument(name = "mcp_call", skip_all, fields(tool = name))]
    /// `project` is the chat's folder, told to the server in `_meta` so a tool can keep
    /// to it, such as a simulator installing only apps built there.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        project: Option<&std::path::Path>,
    ) -> Result<CallOutput> {
        let Value::Object(arguments) = arguments else {
            bail!("arguments must be a JSON object");
        };
        let mut params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        if let Some(project) = project.and_then(|project| project.to_str()) {
            let mut meta = serde_json::Map::new();
            meta.insert(PROJECT_META.into(), Value::String(project.to_owned()));
            params.meta = Some(meta.into());
        }
        let result = tokio::time::timeout(CALL_TIMEOUT, self.service.call_tool(params))
            .await
            .map_err(|_| anyhow!("the server did not answer in {}s", CALL_TIMEOUT.as_secs()))?
            .map_err(|error| anyhow!("the call failed: {error}"))?;
        let mut text = String::new();
        let mut images = Vec::new();
        for part in &result.content {
            if text.len() >= MAX_RESULT_BYTES {
                break;
            }
            if let ContentBlock::Image(image) = part
                && images.len() < MAX_IMAGES
                && let Some(bytes) = image_bytes(&image.data)
            {
                images.push(bytes);
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&content_text(part));
        }
        if text.is_empty()
            && let Some(structured) = &result.structured_content
        {
            text = structured.to_string();
        }
        if result.is_error == Some(true) {
            bail!("{text}");
        }
        Ok(CallOutput { text, images })
    }
}

/// The `_meta` key of the chat's project folder in a tool call.
pub const PROJECT_META: &str = "dev.duskcode/project";

async fn start_child(config: &StdioServerConfig) -> Result<RunningService<RoleClient, ()>> {
    let mut command = match &config.isolation {
        Some(isolation) => {
            let mut line = vec![config.command.clone()];
            line.extend(config.args.iter().cloned());
            isolation.command(&line)?
        }
        None => {
            let mut command = Command::new(&config.command);
            command.args(&config.args);
            command
        }
    };
    command.envs(&config.env);
    let (transport, _) = TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("cannot start {}", config.command))?;
    tokio::time::timeout(START_TIMEOUT, ().serve(transport))
        .await
        .context("the server did not start in time")?
        .map_err(|error| anyhow!("initialize: {error}"))
}

async fn reach(
    config: &HttpServerConfig,
    tokens: Option<TokenFile>,
) -> Result<RunningService<RoleClient, ()>> {
    let url = check_url(&config.url)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| anyhow!("cannot build MCP HTTP client"))?;
    let transport_config = StreamableHttpClientTransportConfig::with_uri(url.as_str())
        .custom_headers(check_headers(config)?);
    let signed_in = match tokens {
        Some(tokens) => oauth::authorized(tokens, &url, client.clone()).await?,
        None => None,
    };
    match signed_in {
        Some(client) => serve(client, transport_config, &url).await,
        None => serve(client, transport_config, &url).await,
    }
}

async fn serve<C: StreamableHttpClient + Send + Sync + 'static>(
    client: C,
    config: StreamableHttpClientTransportConfig,
    url: &Url,
) -> Result<RunningService<RoleClient, ()>> {
    let transport = StreamableHttpClientTransport::with_client(client, config);
    // The error text may quote the URL, which may carry a key in its query.
    tokio::time::timeout(START_TIMEOUT, ().serve(transport))
        .await
        .context("the server did not answer in time")?
        .map_err(|error| {
            let text = error.to_string();
            if text.contains("Auth required") {
                return anyhow!("the server asks to sign in: press l on it in /settings");
            }
            anyhow!("initialize: {}", text.replace(url.as_str(), "<url>"))
        })
}

/// The bytes of a base64 picture, when it is small enough to be a picture at all; what
/// they are is checked by whoever shows them.
fn image_bytes(data: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    if data.len() > crate::state::artifacts::MAX_IMAGE_BYTES * 4 / 3 + 4 {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(data).ok()
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

fn remote_tool(tool: rmcp::model::Tool) -> RemoteTool {
    let hint = |pick: fn(&rmcp::model::ToolAnnotations) -> Option<bool>| {
        tool.annotations.as_ref().and_then(pick) == Some(true)
    };
    RemoteTool {
        repeatable: hint(|a| a.read_only_hint) || hint(|a| a.idempotent_hint),
        read_only: hint(|a| a.read_only_hint),
        name: tool.name.into_owned(),
        description: tool
            .description
            .map(|text| text.chars().take(MAX_DESCRIPTION_CHARS).collect())
            .unwrap_or_default(),
        input_schema: Value::Object((*tool.input_schema).clone()),
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

    /// Whether a server's tool, with these annotations, may be repeated.
    fn repeatable(annotations: Option<Value>) -> bool {
        let mut tool = json!({"name": "t", "inputSchema": {"type": "object"}});
        if let Some(annotations) = annotations {
            tool["annotations"] = annotations;
        }
        remote_tool(serde_json::from_value(tool).unwrap()).repeatable
    }

    #[test]
    fn only_a_read_only_hint_makes_a_tool_read_only() {
        let read_only = |annotations: Value| {
            let tool =
                json!({"name": "t", "inputSchema": {"type": "object"}, "annotations": annotations});
            remote_tool(serde_json::from_value(tool).unwrap()).read_only
        };
        assert!(read_only(json!({"readOnlyHint": true})));
        assert!(!read_only(json!({"idempotentHint": true})));
        assert!(!read_only(json!({"readOnlyHint": false})));
    }

    #[test]
    fn a_server_hint_makes_its_tool_repeatable() {
        assert!(repeatable(Some(json!({"readOnlyHint": true}))));
        assert!(repeatable(Some(json!({"idempotentHint": true}))));
        assert!(!repeatable(None));
        assert!(!repeatable(Some(json!({}))));
        assert!(!repeatable(Some(
            json!({"readOnlyHint": false, "idempotentHint": false})
        )));
    }
}
