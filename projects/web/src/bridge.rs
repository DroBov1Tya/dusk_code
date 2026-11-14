//! One browser's stream: its requests go to the core as any client's would, and the
//! responses, events and status come back as frames.

use anyhow::{Context, Result};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use dusk_core::client::{Client, Hub};
use dusk_protocol::client::{
    CatalogRequest, Hello, PROTOCOL_VERSION, ProjectRequest, Request, RequestFrame, Response,
    ServerFrame, VoiceRequest, WorkRequest,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc, watch};
use tracing::{debug, warn};

/// The largest frame a browser may send. A recording to transcribe is the big one.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;

/// Requests of one browser the core works on at once.
const IN_FLIGHT: usize = 32;

/// The WebSocket close code for data the server does not take.
const UNSUPPORTED: u16 = 1003;
const POLICY: u16 = 1008;

/// How often an open stream is checked for a live session.
const SESSION_CHECK: Duration = Duration::from_secs(20);

/// Carries one browser until it leaves, the core stops or `stop` fires.
pub async fn run(
    mut socket: WebSocket,
    hub: Arc<Hub>,
    mut stop: watch::Receiver<bool>,
    signed_in: impl Fn() -> bool,
) {
    match attach(&hub) {
        Ok((client, events)) => {
            if let Err(error) = carry(&mut socket, client, events, &mut stop, &signed_in).await {
                debug!("web stream ended: {error:#}");
            }
        }
        Err(error) => warn!("cannot attach a browser: {error:#}"),
    }
    let _ = tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Close(None))).await;
}

fn attach(
    hub: &Arc<Hub>,
) -> Result<(
    Arc<Client>,
    mpsc::UnboundedReceiver<dusk_protocol::client::Event>,
)> {
    let mut client = hub.attach(Hello {
        protocol: PROTOCOL_VERSION,
        client: "web".to_owned(),
    })?;
    let events = std::mem::replace(&mut client.events, mpsc::unbounded_channel().1);
    Ok((Arc::new(client), events))
}

async fn carry(
    socket: &mut WebSocket,
    client: Arc<Client>,
    mut events: mpsc::UnboundedReceiver<dusk_protocol::client::Event>,
    stop: &mut watch::Receiver<bool>,
    signed_in: &impl Fn() -> bool,
) -> Result<()> {
    let mut recheck = tokio::time::interval(SESSION_CHECK);
    let mut status = client.status.clone();
    let (replies, mut answers) = mpsc::unbounded_channel::<ServerFrame>();
    let permits = Arc::new(Semaphore::new(IN_FLIGHT));
    let first = ServerFrame::Status(Box::new(status.borrow_and_update().clone()));
    send(socket, &first).await?;
    loop {
        tokio::select! {
            _ = stop.changed() => return Ok(()),
            _ = recheck.tick() => {
                // Signing out, or a session that ran out, ends the stream too.
                if !signed_in() {
                    return refuse(socket, POLICY, "signed out").await;
                }
            }
            message = socket.recv() => {
                let Some(message) = message else { return Ok(()) };
                match message.context("the stream broke")? {
                    Message::Text(text) => match serde_json::from_str::<RequestFrame>(&text) {
                        Ok(frame) => dispatch(frame, text.len(), &client, &permits, &replies),
                        Err(error) => {
                            return refuse(socket, UNSUPPORTED, &format!("not a request: {error}")).await;
                        }
                    },
                    Message::Binary(_) => return refuse(socket, UNSUPPORTED, "text frames only").await,
                    Message::Close(_) => return Ok(()),
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            Some(frame) = answers.recv() => send(socket, &frame).await?,
            event = events.recv() => match event {
                Some(event) => send(socket, &ServerFrame::Event(event)).await?,
                None => return Ok(()),
            },
            changed = status.changed() => {
                changed.context("the core has stopped")?;
                let now = status.borrow_and_update().clone();
                send(socket, &ServerFrame::Status(Box::new(now))).await?;
            }
        }
    }
}

/// Runs a request on its own task, so a slow one does not hold the stream.
fn dispatch(
    frame: RequestFrame,
    size: usize,
    client: &Arc<Client>,
    permits: &Arc<Semaphore>,
    replies: &mpsc::UnboundedSender<ServerFrame>,
) {
    let RequestFrame { id, request } = frame;
    if let Some(refusal) = refused(&request, size) {
        let response = Response::Error(refusal.to_owned());
        let _ = replies.send(ServerFrame::Response { id, response });
        return;
    }
    let Ok(permit) = Arc::clone(permits).try_acquire_owned() else {
        let response = Response::Error("too many requests at once".to_owned());
        let _ = replies.send(ServerFrame::Response { id, response });
        return;
    };
    let (client, replies) = (Arc::clone(client), replies.clone());
    tokio::spawn(async move {
        let response = client.request(request).await;
        drop(permit);
        let _ = replies.send(ServerFrame::Response { id, response });
    });
}

/// The size of an ordinary request; only a recording to transcribe may be larger.
const MAX_REQUEST: usize = 1024 * 1024;

/// What a browser may not ask, though the protocol has it: stopping the core is for the
/// process that started the server, an MCP server's config text holds its keys and tokens,
/// a shell and the files of a project are the desktop app's to open and change, and a
/// request over the ordinary size is only for a recording.
fn refused(request: &Request, size: usize) -> Option<&'static str> {
    match request {
        Request::Shutdown => Some("a browser cannot stop the core"),
        Request::Catalog(CatalogRequest::McpServerText { .. }) => {
            Some("the config of an MCP server holds its secrets; edit it in the desktop app")
        }
        Request::Catalog(CatalogRequest::SaveMcpServer { old: Some(_), .. }) => {
            Some("an existing MCP server is edited in the desktop app")
        }
        Request::Terminal(_) => Some("a terminal opens in the desktop app only"),
        Request::Simulator(_) => Some("the iOS Simulator opens in the desktop app only"),
        Request::Project(ProjectRequest::WriteFile { .. }) => {
            Some("a file is edited in the desktop app only")
        }
        // Sound and pictures are bytes; the core caps a picture, the frame cap the rest.
        Request::Voice(VoiceRequest::Transcribe { .. })
        | Request::Work(WorkRequest::UploadPicture { .. }) => None,
        _ if size > MAX_REQUEST => Some("the request is too large"),
        _ => None,
    }
}

/// How long a browser may take to accept one frame before it is let go: a stalled one
/// would otherwise make the events queue up in memory without end.
const SEND_TIMEOUT: Duration = Duration::from_secs(15);

async fn send(socket: &mut WebSocket, frame: &ServerFrame) -> Result<()> {
    let text = serde_json::to_string(frame).context("cannot write a frame")?;
    tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(text.into())))
        .await
        .context("the browser is too slow")?
        .context("the browser is gone")
}

async fn refuse(socket: &mut WebSocket, code: u16, reason: &str) -> Result<()> {
    let frame = CloseFrame {
        code,
        reason: reason.to_owned().into(),
    };
    let _ = socket.send(Message::Close(Some(frame))).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::client::{TerminalRequest, ThreadRequest};

    #[test]
    fn a_browser_cannot_stop_the_core() {
        assert!(refused(&Request::Shutdown, 10).is_some());
        assert!(refused(&Request::Thread(ThreadRequest::List), 10).is_none());
    }

    #[test]
    fn a_browser_gets_no_shell_and_writes_no_file() {
        let id = || "0190".to_owned();
        for request in [
            TerminalRequest::Open {
                cwd: "/".into(),
                cols: 80,
                rows: 24,
            },
            TerminalRequest::Input {
                id: id(),
                data: b"ls\n".to_vec(),
            },
            TerminalRequest::Resize {
                id: id(),
                cols: 80,
                rows: 24,
            },
            TerminalRequest::Close { id: id() },
        ] {
            assert!(refused(&Request::Terminal(request), 10).is_some());
        }
        let write = Request::Project(ProjectRequest::WriteFile {
            path: "/tmp/x".into(),
            text: String::new(),
            seen: None,
        });
        assert!(refused(&write, 10).is_some());
        let read = Request::Project(ProjectRequest::ReadFile {
            path: "/tmp/x".into(),
        });
        assert!(refused(&read, 10).is_none());
    }

    #[test]
    fn mcp_secrets_stay_in_the_core_and_only_a_recording_may_be_large() {
        let read = Request::Catalog(CatalogRequest::McpServerText { name: "x".into() });
        assert!(refused(&read, 10).is_some());
        let edit = Request::Catalog(CatalogRequest::SaveMcpServer {
            old: Some("x".into()),
            text: String::new(),
        });
        assert!(refused(&edit, 10).is_some());
        let add = Request::Catalog(CatalogRequest::SaveMcpServer {
            old: None,
            text: String::new(),
        });
        assert!(refused(&add, 10).is_none());
        let list = Request::Thread(ThreadRequest::List);
        assert!(refused(&list, MAX_REQUEST + 1).is_some());
        let audio = Request::Voice(VoiceRequest::Transcribe { audio: vec![] });
        assert!(refused(&audio, MAX_REQUEST + 1).is_none());
    }
}
