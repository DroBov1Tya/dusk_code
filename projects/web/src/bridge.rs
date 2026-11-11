//! One browser's stream: its requests go to the core as any client's would, and the
//! responses, events and status come back as frames.

use anyhow::{Context, Result};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use dusk_core::client::{Client, Hub};
use dusk_protocol::client::{
    Hello, PROTOCOL_VERSION, Request, RequestFrame, Response, ServerFrame,
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

/// Carries one browser until it leaves, the core stops or `stop` fires.
pub async fn run(mut socket: WebSocket, hub: Arc<Hub>, mut stop: watch::Receiver<bool>) {
    match attach(&hub) {
        Ok((client, events)) => {
            if let Err(error) = carry(&mut socket, client, events, &mut stop).await {
                debug!("web stream ended: {error:#}");
            }
        }
        Err(error) => warn!("cannot attach a browser: {error:#}"),
    }
    let _ = socket.send(Message::Close(None)).await;
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
) -> Result<()> {
    let mut status = client.status.clone();
    let (replies, mut answers) = mpsc::unbounded_channel::<ServerFrame>();
    let permits = Arc::new(Semaphore::new(IN_FLIGHT));
    let first = ServerFrame::Status(Box::new(status.borrow_and_update().clone()));
    send(socket, &first).await?;
    loop {
        tokio::select! {
            _ = stop.changed() => return Ok(()),
            message = socket.recv() => {
                let Some(message) = message else { return Ok(()) };
                match message.context("the stream broke")? {
                    Message::Text(text) => match serde_json::from_str::<RequestFrame>(&text) {
                        Ok(frame) => dispatch(frame, &client, &permits, &replies),
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
    client: &Arc<Client>,
    permits: &Arc<Semaphore>,
    replies: &mpsc::UnboundedSender<ServerFrame>,
) {
    let RequestFrame { id, request } = frame;
    if let Some(refusal) = refused(&request) {
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

/// What a browser may not ask, though the protocol has it: stopping the core is for the
/// process that started the server.
fn refused(request: &Request) -> Option<&'static str> {
    match request {
        Request::Shutdown => Some("a browser cannot stop the core"),
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
    use dusk_protocol::client::ThreadRequest;

    #[test]
    fn a_browser_cannot_stop_the_core() {
        assert!(refused(&Request::Shutdown).is_some());
        assert!(refused(&Request::Thread(ThreadRequest::List)).is_none());
    }
}
