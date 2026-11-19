//! The pump every client stream shares: requests in, responses, events and status out.
//! A transport only moves text frames (`Frames`); what is carried, and what is refused,
//! is the same for a browser's WebSocket and a local client's JSON lines.

use anyhow::{Context, Result};
use dusk_core::client::Client;
use dusk_protocol::client::{
    Event, Request, RequestFrame, Response, ServerFrame, VoiceRequest, WorkRequest,
};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc, watch};
use tracing::debug;

/// The largest frame a client may send. A recording to transcribe is the big one.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;

/// Requests of one client the core works on at once.
const IN_FLIGHT: usize = 32;

/// How often an open stream is checked for a live session.
const SESSION_CHECK: Duration = Duration::from_secs(20);

/// How long a client may take to accept one frame before it is let go: a stalled one
/// would otherwise make the events queue up in memory without end.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(15);

/// The reason `Frames::close` gets when the session is over.
pub const SIGNED_OUT: &str = "signed out";

/// The reason when a frame is not a request; on the wire as it is, never with the parser's text.
pub const NOT_A_REQUEST: &str = "not a request";

/// The size of an ordinary request; only a recording or a picture may be larger.
pub const MAX_REQUEST: usize = 1024 * 1024;

/// A stream of text frames: a WebSocket's messages, or the lines of a socket.
pub trait Frames: Send {
    /// The next frame, or none when the peer left.
    fn next(&mut self) -> impl Future<Output = Result<Option<String>>> + Send;
    fn send(&mut self, text: String) -> impl Future<Output = Result<()>> + Send;
    /// Ends the stream, telling the peer why where the transport can: a WebSocket in its
    /// close frame, JSON lines in a last `{"Closed":{"reason":..}}` line.
    fn close(&mut self, reason: &str) -> impl Future<Output = ()> + Send;
}

/// Carries one client until it leaves, the core stops, `stop` fires or `alive` turns false.
pub async fn pump(
    frames: &mut impl Frames,
    client: Arc<Client>,
    mut events: mpsc::UnboundedReceiver<Event>,
    stop: &mut watch::Receiver<bool>,
    alive: &impl Fn() -> bool,
) -> Result<()> {
    let mut recheck = tokio::time::interval(SESSION_CHECK);
    let mut status = client.status.clone();
    let (replies, mut answers) = mpsc::unbounded_channel::<ServerFrame>();
    let permits = Arc::new(Semaphore::new(IN_FLIGHT));
    // A scope that may not read the status gets none: its status is a blank.
    let reads_status = client.reads_status();
    if reads_status {
        let first = ServerFrame::Status(Box::new(status.borrow_and_update().clone()));
        send(frames, &first).await?;
    }
    loop {
        tokio::select! {
            _ = stop.changed() => return Ok(()),
            _ = recheck.tick() => {
                // Signing out, or a session that ran out, ends the stream too.
                if !alive() {
                    frames.close(SIGNED_OUT).await;
                    return Ok(());
                }
            }
            frame = frames.next() => {
                let Some(text) = frame.context("the stream broke")? else { return Ok(()) };
                match serde_json::from_str::<RequestFrame>(&text) {
                    Ok(frame) => dispatch(frame, text.len(), &client, &permits, &replies),
                    Err(error) => {
                        debug!("not a request: {error}");
                        frames.close(NOT_A_REQUEST).await;
                        return Ok(());
                    }
                }
            }
            Some(frame) = answers.recv() => send(frames, &frame).await?,
            event = events.recv() => match event {
                Some(event) => send(frames, &ServerFrame::Event(event)).await?,
                None => return Ok(()),
            },
            changed = status.changed(), if reads_status => {
                changed.context("the core has stopped")?;
                let now = status.borrow_and_update().clone();
                send(frames, &ServerFrame::Status(Box::new(now))).await?;
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
    if too_large(&request, size) {
        let response = Response::Error("the request is too large".to_owned());
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

/// Whether the frame is over the ordinary size. Sound and pictures are bytes: the core
/// caps a picture, the frame cap the rest.
pub fn too_large(request: &Request, size: usize) -> bool {
    let bytes = matches!(
        request,
        Request::Voice(VoiceRequest::Transcribe { .. })
            | Request::Work(WorkRequest::UploadPicture { .. })
    );
    !bytes && size > MAX_REQUEST
}

async fn send(frames: &mut impl Frames, frame: &ServerFrame) -> Result<()> {
    let text = serde_json::to_string(frame).context("cannot write a frame")?;
    send_text(frames, text).await
}

async fn send_text(frames: &mut impl Frames, text: String) -> Result<()> {
    tokio::time::timeout(SEND_TIMEOUT, frames.send(text))
        .await
        .context("the client is too slow")?
        .context("the client is gone")
}
