//! Incremental Server-Sent Events decoder (`text/event-stream`), limited to `data` fields.
//! Lines end in `\n` or `\r\n`; a lone `\r` terminator is not supported.

use anyhow::{Result, anyhow, ensure};

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
    fn decodes_events_separated_by_blank_lines() {
        let events = decode_all(&[b"data: one\n\ndata: two\n\n"]).unwrap();
        assert_eq!(events, ["one", "two"]);
    }

    #[test]
    fn ignores_comments_and_other_fields_with_crlf() {
        let events = decode_all(&[b": ping\r\nevent: x\r\nid: 1\r\ndata:raw\r\n\r\n"]).unwrap();
        assert_eq!(events, ["raw"]);
    }

    #[test]
    fn joins_multiline_data_with_newlines() {
        let events = decode_all(&[b"data: a\ndata: b\n\n"]).unwrap();
        assert_eq!(events, ["a\nb"]);
    }

    #[test]
    fn keeps_incomplete_event_pending() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(b"data: par").unwrap().is_empty());
        assert!(decoder.feed(b"tial\n").unwrap().is_empty());
        assert_eq!(decoder.feed(b"\n").unwrap(), ["partial"]);
    }

    #[test]
    fn rejects_oversized_line() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(&vec![b'a'; MAX_LINE_BYTES + 1]).is_err());
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(decode_all(&[b"data: \xff\n\n"]).is_err());
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
