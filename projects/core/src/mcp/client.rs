//! One MCP server over stdio: the core starts it as a child process and they exchange
//! JSON-RPC messages, one per line. The child's stderr is dropped, since the client may
//! be drawing on the terminal.

use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Mutex as AsyncMutex, oneshot},
};
use tracing::warn;

use crate::config::McpServerConfig;

const PROTOCOL_VERSION: &str = "2025-06-18";
/// A server that sends a longer line is broken or hostile; it is stopped.
const MAX_LINE_BYTES: u64 = 8 * 1024 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Pages of tools a server may list, so a server cannot keep the start going forever.
const MAX_TOOL_PAGES: usize = 20;

/// Answers waiting for their request's ID.
type Waiting = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct McpClient {
    stdin: AsyncMutex<ChildStdin>,
    waiting: Waiting,
    next_id: AtomicU64,
    /// Killed when the client is dropped.
    _child: Child,
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
    /// Starts the server and agrees on the protocol with it.
    pub async fn start(config: &McpServerConfig) -> Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("cannot start {}", config.command))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let waiting = Waiting::default();
        let client = Self {
            stdin: AsyncMutex::new(stdin),
            waiting: Arc::clone(&waiting),
            next_id: AtomicU64::new(1),
            _child: child,
        };
        tokio::spawn(read_messages(stdout, waiting));
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "dusk", "version": env!("CARGO_PKG_VERSION") },
        });
        client
            .request("initialize", params, START_TIMEOUT)
            .await
            .context("initialize")?;
        client
            .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await?;
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
        let (answer, answered) = oneshot::channel();
        lock(&self.waiting).insert(id, answer);
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(error) = self.send(message).await {
            lock(&self.waiting).remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(timeout, answered).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => bail!("the server stopped"),
            Err(_) => {
                lock(&self.waiting).remove(&id);
                bail!(
                    "the server did not answer {method} in {}s",
                    timeout.as_secs()
                )
            }
        }
    }

    async fn send(&self, message: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&message)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&line).await.context("the server stopped")?;
        stdin.flush().await.context("the server stopped")
    }
}

/// Hands every answer to its request until the server closes its output or misbehaves;
/// then every request still waiting fails.
async fn read_messages(stdout: ChildStdout, waiting: Waiting) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut line)
            .await;
        match read {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() as u64 >= MAX_LINE_BYTES && !line.ends_with(b"\n") => {
                warn!("MCP server sent a line over {MAX_LINE_BYTES} bytes; stopped reading");
                break;
            }
            Ok(_) => {}
        }
        // Servers may log or send notifications and requests of their own; only answers
        // to our requests matter here.
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            continue;
        };
        if message.get("method").is_some() {
            continue;
        }
        let Some(answer) = lock(&waiting).remove(&id) else {
            continue;
        };
        let _ = answer.send(answer_of(message));
    }
    lock(&waiting).clear();
}

fn answer_of(message: Value) -> Result<Value> {
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

// Only inserts and removes happen under these locks, so a panic cannot leave a map
// half-changed.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        assert!(answer_of(error).is_err());
    }
}
