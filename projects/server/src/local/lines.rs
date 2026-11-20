//! JSON lines over any byte stream: one frame per line, the transport of the local API.

use crate::stream::{Frames, SEND_TIMEOUT};
use anyhow::{Result, bail};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Frames as lines of text over a stream.
pub struct Lines<S> {
    stream: S,
    max_line: usize,
    /// What was read and not yet taken as a line; kept across calls so a cancelled `next`
    /// loses nothing.
    buffer: Vec<u8>,
    /// Where in `buffer` the search for a newline goes on.
    scanned: usize,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Lines<S> {
    pub fn new(stream: S, max_line: usize) -> Self {
        Self {
            stream,
            max_line,
            buffer: Vec::new(),
            scanned: 0,
        }
    }

    /// Changes the longest line taken from now on: the handshake's lines are short, the
    /// stream's may carry a recording.
    pub fn set_max_line(&mut self, max_line: usize) {
        self.max_line = max_line;
    }

    fn take_line(&mut self) -> Result<Option<String>> {
        let found = self.buffer[self.scanned..].iter().position(|b| *b == b'\n');
        let Some(at) = found.map(|at| at + self.scanned) else {
            self.scanned = self.buffer.len();
            if self.buffer.len() > self.max_line {
                bail!("a line is over {} bytes", self.max_line);
            }
            return Ok(None);
        };
        if at > self.max_line {
            bail!("a line is over {} bytes", self.max_line);
        }
        let rest = self.buffer.split_off(at + 1);
        let mut line = std::mem::replace(&mut self.buffer, rest);
        self.scanned = 0;
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Ok(Some(String::from_utf8(line)?))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Frames for Lines<S> {
    async fn next(&mut self) -> Result<Option<String>> {
        loop {
            match self.take_line() {
                Ok(Some(line)) if line.is_empty() => continue,
                Ok(Some(line)) => return Ok(Some(line)),
                Ok(None) => {}
                Err(error) => {
                    self.close(&error.to_string()).await;
                    return Err(error);
                }
            }
            let mut chunk = [0u8; 16 * 1024];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return match self.buffer.is_empty() {
                    true => Ok(None),
                    false => Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                };
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    async fn send(&mut self, text: String) -> Result<()> {
        let mut line = text.into_bytes();
        line.push(b'\n');
        self.stream.write_all(&line).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn close(&mut self, reason: &str) {
        let closed = serde_json::json!({ "Closed": { "reason": reason } });
        let _ = tokio::time::timeout(SEND_TIMEOUT, self.send(closed.to_string())).await;
        let _ = self.stream.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::pump;
    use dusk_core::{
        app::{Paths, start_hub},
        client::Scope,
    };
    use dusk_protocol::client::{Event, Hello, Request, RequestFrame, ServerFrame, ThreadRequest};
    use std::{sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt, BufReader, duplex},
        sync::{mpsc, watch},
    };

    async fn attached(
        name: &str,
        scope: Scope,
    ) -> (
        Arc<dusk_core::client::Client>,
        mpsc::UnboundedReceiver<Event>,
    ) {
        let root = std::env::temp_dir().join(format!("dusk-lines-{}-{name}", std::process::id()));
        let paths = Paths::new(root.join("config"), root.join("work"));
        let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
        let mut client = hub.attach(Hello::new("test"), scope).unwrap();
        let events = std::mem::replace(&mut client.events, mpsc::unbounded_channel().1);
        (Arc::new(client), events)
    }

    #[tokio::test]
    async fn lines_round_trip_a_request_frame() {
        let (client, events) = attached("round", Scope::full()).await;
        let (near, far) = duplex(1 << 20);
        let (_stop_tx, mut stop) = watch::channel(false);
        let pumping = tokio::spawn(async move {
            let mut lines = Lines::new(near, 1 << 20);
            pump(&mut lines, client, events, &mut stop, &|| true).await
        });
        let (read, mut write) = tokio::io::split(far);
        let mut read = BufReader::new(read);
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<ServerFrame>(&line).unwrap(),
            ServerFrame::Status(_)
        ));
        let frame = RequestFrame {
            id: 7,
            request: Request::Thread(ThreadRequest::List),
        };
        let mut text = serde_json::to_string(&frame).unwrap();
        text.push('\n');
        write.write_all(text.as_bytes()).await.unwrap();
        loop {
            line.clear();
            read.read_line(&mut line).await.unwrap();
            if let ServerFrame::Response { id, .. } = serde_json::from_str(&line).unwrap() {
                assert_eq!(id, 7);
                break;
            }
        }
        // Not a request: told so, and the stream ends.
        write.write_all(b"{\"nope\":1}\n").await.unwrap();
        line.clear();
        read.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end(), r#"{"Closed":{"reason":"not a request"}}"#);
        assert!(pumping.await.unwrap().is_ok());
        line.clear();
        assert_eq!(read.read_line(&mut line).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_line_over_the_cap_closes() {
        let (near, mut far) = duplex(1 << 16);
        let mut lines = Lines::new(near, 16);
        far.write_all(b"ok\n").await.unwrap();
        assert_eq!(lines.next().await.unwrap().as_deref(), Some("ok"));
        far.write_all(&[b'x'; 64]).await.unwrap();
        assert!(lines.next().await.is_err());
        // The peer is told why, then the stream ends.
        let mut rest = String::new();
        far.read_to_string(&mut rest).await.unwrap();
        assert!(
            rest.starts_with(r#"{"Closed":{"reason":"a line is over 16 bytes"#),
            "{rest}"
        );
    }

    #[tokio::test]
    async fn a_line_that_is_not_text_or_is_cut_off_ends_the_stream() {
        let (near, mut far) = duplex(1 << 16);
        let mut lines = Lines::new(near, 1 << 10);
        far.write_all(b"\xff\xfe\n").await.unwrap();
        assert!(lines.next().await.is_err());
        let mut rest = String::new();
        far.read_to_string(&mut rest).await.unwrap();
        assert!(rest.starts_with(r#"{"Closed":"#), "{rest}");

        let (near, mut far) = duplex(1 << 16);
        let mut lines = Lines::new(near, 1 << 10);
        far.write_all(b"half").await.unwrap();
        drop(far);
        assert!(lines.next().await.is_err());
    }

    #[tokio::test]
    async fn an_ordinary_request_over_a_megabyte_is_refused_over_lines() {
        let (client, events) = attached("large", Scope::full()).await;
        let (near, far) = duplex(1 << 22);
        let (_stop_tx, mut stop) = watch::channel(false);
        tokio::spawn(async move {
            let mut lines = Lines::new(near, 32 << 20);
            let _ = pump(&mut lines, client, events, &mut stop, &|| true).await;
        });
        let (read, mut write) = tokio::io::split(far);
        let mut read = BufReader::new(read);
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        let big = RequestFrame {
            id: 3,
            request: Request::Thread(ThreadRequest::Send {
                thread_id: "t".into(),
                text: "x".repeat(2 << 20),
                now: false,
            }),
        };
        let mut text = serde_json::to_string(&big).unwrap();
        text.push('\n');
        write.write_all(text.as_bytes()).await.unwrap();
        loop {
            line.clear();
            read.read_line(&mut line).await.unwrap();
            if let ServerFrame::Response { id, response } = serde_json::from_str(&line).unwrap() {
                assert_eq!(id, 3);
                assert!(
                    matches!(response, dusk_protocol::client::Response::Error(m) if m.contains("too large"))
                );
                break;
            }
        }
    }

    /// The first line a stream of this scope carries, if it carries one.
    async fn first_line(scope: Scope, name: &str) -> Option<String> {
        let (client, events) = attached(name, scope).await;
        let (near, far) = duplex(1 << 20);
        let (_stop_tx, mut stop) = watch::channel(false);
        tokio::spawn(async move {
            let mut lines = Lines::new(near, 1 << 20);
            let _ = pump(&mut lines, client, events, &mut stop, &|| true).await;
        });
        let mut read = BufReader::new(far);
        let mut line = String::new();
        let waited = tokio::time::timeout(Duration::from_millis(500), read.read_line(&mut line));
        waited.await.ok().map(|_| line)
    }

    #[tokio::test]
    async fn only_a_scope_that_reads_the_status_gets_one_first() {
        for (scope, name) in [(Scope::full(), "full"), (Scope::web(), "web")] {
            let line = first_line(scope, name).await.expect("a first frame");
            assert!(line.starts_with(r#"{"Status""#), "{line}");
        }
        let areas: &'static [dusk_core::client::Area] = &[dusk_core::client::Area::ThreadRead];
        let threads = Scope::threads(["t".to_owned()], areas);
        assert!(!threads.reads_status());
        assert_eq!(first_line(threads, "threads").await, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_reader_that_stalls_is_dropped() {
        let (client, _unused) = attached("stall", Scope::full()).await;
        let (events_tx, events) = mpsc::unbounded_channel();
        let (near, far) = duplex(8192);
        let (_stop_tx, mut stop) = watch::channel(false);
        let pumping = tokio::spawn(async move {
            let mut lines = Lines::new(near, 1 << 20);
            pump(&mut lines, client, events, &mut stop, &|| true).await
        });
        // The reader takes the first Status, then never reads again.
        let mut read = BufReader::new(far);
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        assert!(line.starts_with(r#"{"Status""#));
        tokio::time::pause();
        for n in 0..50 {
            let _ = events_tx.send(Event::Terminal {
                id: format!("t{n}"),
                data: vec![7; 4096],
            });
        }
        tokio::time::sleep(Duration::from_secs(16)).await;
        assert!(pumping.await.unwrap().is_err());
    }
}
