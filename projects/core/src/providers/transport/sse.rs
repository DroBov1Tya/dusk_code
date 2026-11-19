//! Incremental Server-Sent Events decoder (`text/event-stream`), limited to `data` fields.
//! Lines end in `\n` or `\r\n`; a lone `\r` terminator is not supported.
//!
//! `SseStream` reads a provider's answer with it, chunk by chunk, and hands each event's
//! data to the provider's `EventParser`.

use std::collections::VecDeque;

use anyhow::{Result, anyhow, ensure};
use reqwest::Response;

use crate::providers::{output::ModelEvent, transport::http};

const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct SseDecoder {
    line: Vec<u8>,
    data: Option<String>,
}

impl SseDecoder {
    /// Feeds raw bytes and returns the `data` payload of every event they complete.
    pub fn feed(&mut self, mut bytes: &[u8]) -> Result<Vec<String>> {
        let mut events = Vec::new();
        while let Some(end) = bytes.iter().position(|&byte| byte == b'\n') {
            self.append_to_line(&bytes[..end])?;
            let line = std::mem::take(&mut self.line);
            events.extend(self.process_line(&line)?);
            bytes = &bytes[end + 1..];
        }
        self.append_to_line(bytes)?;
        Ok(events)
    }

    fn append_to_line(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_LINE_BYTES - self.line.len(),
            "provider stream line is too long"
        );
        self.line.extend_from_slice(bytes);
        Ok(())
    }

    fn process_line(&mut self, line: &[u8]) -> Result<Option<String>> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        // Lines split only at '\n', so a complete line never cuts a UTF-8 sequence.
        let line =
            std::str::from_utf8(line).map_err(|_| anyhow!("provider stream is not UTF-8"))?;
        if line.is_empty() {
            return Ok(self.data.take());
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        if field == "data" {
            self.append_data(value)?;
        }
        Ok(None)
    }

    fn append_data(&mut self, value: &str) -> Result<()> {
        match &mut self.data {
            Some(data) => {
                ensure!(
                    data.len() + 1 + value.len() <= MAX_EVENT_BYTES,
                    "provider stream event is too large"
                );
                data.push('\n');
                data.push_str(value);
            }
            None => self.data = Some(value.to_owned()),
        }
        Ok(())
    }
}

/// A provider's own reading of its events: what each `data` payload means, and when
/// the answer is whole. Network-independent, so a protocol can be tested from raw bytes.
pub trait EventParser: Send {
    fn decoder(&mut self) -> &mut SseDecoder;

    /// The model events of one event's `data`.
    fn parse(&mut self, data: &str) -> Result<Vec<ModelEvent>>;

    /// Whether the answer is whole; later events are not read.
    fn done(&self) -> bool;

    /// The stream ended: an error when the answer was not whole.
    fn finish(&mut self) -> Result<()>;

    /// Runs after every chunk was fed, also when feeding it failed.
    fn fed(&mut self) {}

    /// The model events in `bytes`, up to the one that makes the answer whole.
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder().feed(bytes)? {
            if self.done() {
                break;
            }
            events.extend(self.parse(&data)?);
        }
        Ok(events)
    }
}

/// Events of one streamed answer, read lazily from the network.
pub struct SseStream<P> {
    response: Response,
    parser: P,
    pending: VecDeque<ModelEvent>,
    finished: bool,
}

impl<P: EventParser> SseStream<P> {
    pub fn new(response: Response, parser: P) -> Self {
        Self {
            response,
            parser,
            pending: VecDeque::new(),
            finished: false,
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            if self.finished {
                return Ok(None);
            }
            // Mark first so an error also ends the stream.
            self.finished = true;
            let chunk = self.response.chunk().await.map_err(http::cut)?;
            match chunk {
                Some(bytes) => {
                    let parsed = self.parser.feed(&bytes);
                    self.parser.fed();
                    self.pending.extend(parsed?);
                    self.finished = self.parser.done();
                }
                None => self.parser.finish()?,
            }
        }
    }
}

impl<P: EventParser> super::super::output::ChatStream for SseStream<P> {
    fn next_event(&mut self) -> crate::BoxFuture<'_, Option<ModelEvent>> {
        Box::pin(SseStream::next_event(self))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn decode_all(chunks: &[&[u8]]) -> Result<Vec<String>> {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(decoder.feed(chunk)?);
        }
        Ok(events)
    }

    #[test]
    fn rejects_oversized_line() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(&vec![b'a'; MAX_LINE_BYTES + 1]).is_err());
    }

    proptest! {
        #[test]
        fn chunking_does_not_change_events(
            payloads in prop::collection::vec("[^\r\n]{0,40}", 0..8),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
        ) {
            let encoded: String = payloads.iter().map(|p| format!("data: {p}\r\n\r\n")).collect();
            let bytes = encoded.as_bytes();
            let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(bytes.len() + 1)).collect();
            points.sort_unstable();
            let mut chunks = Vec::new();
            let mut start = 0;
            for point in points {
                chunks.push(&bytes[start..point]);
                start = point;
            }
            chunks.push(&bytes[start..]);
            prop_assert_eq!(decode_all(&chunks).unwrap(), payloads);
        }

        #[test]
        fn arbitrary_bytes_never_panic(
            chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 0..16),
        ) {
            let mut decoder = SseDecoder::default();
            for chunk in &chunks {
                if decoder.feed(chunk).is_err() {
                    break;
                }
            }
        }
    }
}
