//! The link of a client to a core over a socket: one line of JSON per frame, a reader
//! task that routes what the core sends, and a writer task that keeps requests in order.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use dusk_protocol::client::{Event, Request, RequestFrame, Response, ServerFrame, Status};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

/// The longest line the core may send: its biggest answers are pictures and transcripts.
const MAX_LINE: u64 = 32 * 1024 * 1024;

/// The calls waiting for an answer; `None` once the core is gone, so a later call fails
/// at once instead of waiting for an answer that cannot come.
type Pending = Arc<Mutex<Option<HashMap<u64, oneshot::Sender<Response>>>>>;

pub(super) struct Link {
    pending: Pending,
    next: AtomicU64,
    lines: mpsc::UnboundedSender<(u64, String)>,
    tasks: [JoinHandle<()>; 2],
}

impl Link {
    pub(super) fn start(
        read: impl AsyncBufRead + Unpin + Send + 'static,
        write: impl AsyncWrite + Unpin + Send + 'static,
        events: mpsc::UnboundedSender<Event>,
        status: watch::Sender<Status>,
    ) -> Link {
        let pending: Pending = Arc::new(Mutex::new(Some(HashMap::new())));
        let (lines, queued) = mpsc::unbounded_channel();
        let reader = tokio::spawn(read_frames(read, Arc::clone(&pending), events, status));
        let writer = tokio::spawn(write_lines(write, queued, Arc::clone(&pending)));
        Link {
            pending,
            next: AtomicU64::new(1),
            lines,
            tasks: [reader, writer],
        }
    }

    /// Queues `request`; the receiver is dropped unanswered when the core is gone.
    pub(super) fn send(&self, request: Request) -> oneshot::Receiver<Response> {
        let (reply, answer) = oneshot::channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let Ok(mut line) = serde_json::to_string(&RequestFrame { id, request }) else {
            return answer;
        };
        line.push('\n');
        {
            let mut guard = lock(&self.pending);
            let Some(pending) = guard.as_mut() else {
                return answer;
            };
            pending.insert(id, reply);
        }
        if self.lines.send((id, line)).is_err() {
            forget(&self.pending, id);
        }
        answer
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // Ending the tasks drops both halves of the socket, which is how the core sees us go.
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn lock(
    pending: &Pending,
) -> std::sync::MutexGuard<'_, Option<HashMap<u64, oneshot::Sender<Response>>>> {
    pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn forget(pending: &Pending, id: u64) {
    if let Some(map) = lock(pending).as_mut() {
        map.remove(&id);
    }
}

/// Fails every waiting call and every later one.
fn close(pending: &Pending) {
    lock(pending).take();
}

async fn read_frames(
    mut read: impl AsyncBufRead + Unpin,
    pending: Pending,
    events: mpsc::UnboundedSender<Event>,
    status: watch::Sender<Status>,
) {
    while let Some(line) = next_line(&mut read).await {
        // A line that is no frame means the other end is not our core: stop listening.
        let Ok(frame) = serde_json::from_str::<ServerFrame>(&line) else {
            break;
        };
        match frame {
            ServerFrame::Response { id, response } => {
                let reply = lock(&pending).as_mut().and_then(|map| map.remove(&id));
                if let Some(reply) = reply {
                    let _ = reply.send(response);
                }
            }
            ServerFrame::Event(event) => {
                let _ = events.send(event);
            }
            ServerFrame::Status(now) => {
                status.send_replace(*now);
            }
        }
    }
    close(&pending);
}

/// The next line, or `None` at the end of the stream, on a read error or a line too long.
async fn next_line<R: AsyncBufRead + Unpin>(read: &mut R) -> Option<String> {
    let mut line = String::new();
    let mut limited = read.take(MAX_LINE);
    let count = limited.read_line(&mut line).await.ok()?;
    (count > 0 && line.ends_with('\n')).then_some(line)
}

async fn write_lines(
    mut write: impl AsyncWrite + Unpin,
    mut queued: mpsc::UnboundedReceiver<(u64, String)>,
    pending: Pending,
) {
    while let Some((id, line)) = queued.recv().await {
        let sent = async {
            write.write_all(line.as_bytes()).await?;
            write.flush().await
        };
        if sent.await.is_err() {
            forget(&pending, id);
            close(&pending);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::client::{Request, ThreadRequest};
    use tokio::io::{BufReader, duplex};

    use super::*;
    use crate::client::Client;

    async fn send_frame(write: &mut (impl AsyncWrite + Unpin), frame: &ServerFrame) {
        let mut line = serde_json::to_string(frame).unwrap();
        line.push('\n');
        write.write_all(line.as_bytes()).await.unwrap();
    }

    #[tokio::test]
    async fn a_remote_client_calls_and_hears_events() {
        let (near, far) = duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(near);
        let (core_read, mut core_write) = tokio::io::split(far);
        let ended = Event::Ended {
            thread_id: "t1".to_owned(),
        };
        let core = tokio::spawn({
            let ended = ended.clone();
            async move {
                let mut read = BufReader::new(core_read);
                let mut line = String::new();
                read.read_line(&mut line).await.unwrap();
                let frame: RequestFrame = serde_json::from_str(&line).unwrap();
                assert_eq!(frame.request, Request::Thread(ThreadRequest::List));
                send_frame(&mut core_write, &ServerFrame::Event(ended)).await;
                let response = Response::Threads(vec![]);
                send_frame(
                    &mut core_write,
                    &ServerFrame::Response {
                        id: frame.id,
                        response,
                    },
                )
                .await;
                // Dropping both halves closes the socket.
            }
        });
        let mut client = Client::over(BufReader::new(client_read), client_write).await;
        assert_eq!(client.threads().list().await.unwrap(), vec![]);
        assert_eq!(client.events.recv().await, Some(ended));
        core.await.unwrap();
        let error = client.threads().list().await.unwrap_err();
        assert_eq!(error.to_string(), "the core has stopped");
        assert_eq!(client.events.recv().await, None);
        assert!(client.hub().is_none());
    }
}
