//! One browser's stream: its requests go to the core as any client's would, and the
//! responses, events and status come back as frames.

use crate::stream::{self, Frames, SEND_TIMEOUT, SIGNED_OUT, pump};
use anyhow::{Context, Result};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use dusk_core::client::{Client, Hub, Scope};
use dusk_protocol::client::{Event, Hello};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

/// The largest frame a browser may send.
pub const MAX_FRAME: usize = stream::MAX_FRAME;

/// The WebSocket close codes for data the server does not take, and for a session over.
const UNSUPPORTED: u16 = 1003;
const POLICY: u16 = 1008;

/// Carries one browser until it leaves, the core stops or `stop` fires.
pub async fn run(
    mut socket: WebSocket,
    hub: Arc<Hub>,
    mut stop: watch::Receiver<bool>,
    signed_in: impl Fn() -> bool,
) {
    match attach(&hub) {
        Ok((client, events)) => {
            let mut frames = Socket(&mut socket);
            if let Err(error) = pump(&mut frames, client, events, &mut stop, &signed_in).await {
                debug!("web stream ended: {error:#}");
            }
        }
        Err(error) => warn!("cannot attach a browser: {error:#}"),
    }
    let _ = tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Close(None))).await;
}

fn attach(hub: &Arc<Hub>) -> Result<(Arc<Client>, mpsc::UnboundedReceiver<Event>)> {
    // What a browser may ask is the core's to check: it is attached with the web scope.
    let mut client = hub.attach(Hello::new("web"), Scope::web())?;
    let events = std::mem::replace(&mut client.events, mpsc::unbounded_channel().1);
    Ok((Arc::new(client), events))
}

/// A browser's WebSocket as text frames.
struct Socket<'a>(&'a mut WebSocket);

impl Frames for Socket<'_> {
    async fn next(&mut self) -> Result<Option<String>> {
        loop {
            let Some(message) = self.0.recv().await else {
                return Ok(None);
            };
            match message.context("the stream broke")? {
                Message::Text(text) => return Ok(Some(text.as_str().to_owned())),
                Message::Binary(_) => {
                    self.close("text frames only").await;
                    return Ok(None);
                }
                Message::Close(_) => return Ok(None),
                Message::Ping(_) | Message::Pong(_) => {}
            }
        }
    }

    async fn send(&mut self, text: String) -> Result<()> {
        Ok(self.0.send(Message::Text(text.into())).await?)
    }

    async fn close(&mut self, reason: &str) -> () {
        let code = if reason == SIGNED_OUT {
            POLICY
        } else {
            UNSUPPORTED
        };
        let frame = CloseFrame {
            code,
            reason: reason.to_owned().into(),
        };
        let _ = tokio::time::timeout(SEND_TIMEOUT, self.0.send(Message::Close(Some(frame)))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{MAX_REQUEST, too_large};
    use dusk_protocol::client::{CatalogRequest, ProjectRequest, TerminalRequest, ThreadRequest};
    use dusk_protocol::client::{Request, VoiceRequest};

    /// Whether the core refuses the request of a browser.
    fn refused(request: &Request) -> bool {
        !Scope::web().permits(request)
    }

    #[test]
    fn a_browser_cannot_stop_the_core() {
        assert!(refused(&Request::Shutdown));
        assert!(!refused(&Request::Thread(ThreadRequest::List)));
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
            assert!(refused(&Request::Terminal(request)));
        }
        let write = Request::Project(ProjectRequest::WriteFile {
            path: "/tmp/x".into(),
            text: String::new(),
            seen: None,
        });
        assert!(refused(&write));
        let read = Request::Project(ProjectRequest::ReadFile {
            path: "/tmp/x".into(),
        });
        assert!(!refused(&read));
    }

    #[test]
    fn mcp_secrets_stay_in_the_core_and_only_a_recording_may_be_large() {
        let read = Request::Catalog(CatalogRequest::McpServerText { name: "x".into() });
        assert!(refused(&read));
        let edit = Request::Catalog(CatalogRequest::SaveMcpServer {
            old: Some("x".into()),
            text: String::new(),
        });
        assert!(refused(&edit));
        let add = Request::Catalog(CatalogRequest::SaveMcpServer {
            old: None,
            text: String::new(),
        });
        assert!(!refused(&add));
        let list = Request::Thread(ThreadRequest::List);
        assert!(too_large(&list, MAX_REQUEST + 1));
        assert!(!too_large(&list, MAX_REQUEST));
        let audio = Request::Voice(VoiceRequest::Transcribe { audio: vec![] });
        assert!(!too_large(&audio, MAX_REQUEST + 1));
        assert!(!refused(&audio));
    }
}
