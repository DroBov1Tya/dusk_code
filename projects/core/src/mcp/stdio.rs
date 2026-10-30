//! A server the core starts as a child process: JSON-RPC messages go to its stdin and come
//! back on its stdout, one per line. The child's stderr is dropped, since the client may
//! be drawing on the terminal.

use std::{
    collections::HashMap,
    process::Stdio,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Mutex as AsyncMutex, oneshot},
};
use tracing::warn;

use super::lock;
use crate::config::StdioServerConfig;

/// A server that sends a longer line is broken or hostile; it is no longer read.
const MAX_LINE_BYTES: u64 = 8 * 1024 * 1024;

/// Answers waiting for their request's ID.
type Waiting = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct StdioTransport {
    stdin: AsyncMutex<ChildStdin>,
    waiting: Waiting,
    /// Killed when the transport is dropped.
    _child: Child,
}

impl StdioTransport {
    pub fn start(config: &StdioServerConfig) -> Result<Self> {
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
        tokio::spawn(read_messages(stdout, Arc::clone(&waiting)));
        Ok(Self {
            stdin: AsyncMutex::new(stdin),
            waiting,
            _child: child,
        })
    }

    /// Sends a request and waits for the message that answers `id`.
    pub async fn request(&self, id: u64, message: &Value) -> Result<Value> {
        let (answer, answered) = oneshot::channel();
        lock(&self.waiting).insert(id, answer);
        if let Err(error) = self.send(message).await {
            lock(&self.waiting).remove(&id);
            return Err(error);
        }
        // A request given up on leaves its entry until the answer comes or the server
        // stops; either removes it.
        match answered.await {
            Ok(answer) => Ok(answer),
            Err(_) => bail!("the server stopped"),
        }
    }

    pub async fn send(&self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(message)?;
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
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let Some(id) = super::answer_id(&message) else {
            continue;
        };
        if let Some(answer) = lock(&waiting).remove(&id) {
            let _ = answer.send(message);
        }
    }
    lock(&waiting).clear();
}
