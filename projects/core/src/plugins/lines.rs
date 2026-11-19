//! Lines of JSON from a plugin's own process. That output is untrusted, so a line is
//! read with a hard cap and never held past it, however long the process goes on
//! without a line break.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// What the next read of a process brought.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// One line, without its line break.
    Full(Vec<u8>),
    /// A line grew past the cap. What was read of it is gone; the next read skips
    /// the rest of it, so the caller may go on or stop the process.
    TooLong,
    /// The process closed its output.
    End,
}

/// Reads lines of at most `max` bytes. It keeps a part read across a cancel, so it may
/// sit in a `select!`.
pub(crate) struct Lines {
    line: Vec<u8>,
    max: usize,
    /// Within a line already reported too long, read up to its end and dropped.
    skipping: bool,
}

impl Lines {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            line: Vec::new(),
            max,
            skipping: false,
        }
    }

    /// The next line. A last line without a break still counts at the end.
    pub(crate) async fn next(
        &mut self,
        reader: &mut (impl AsyncBufRead + Unpin),
    ) -> io::Result<Line> {
        loop {
            let buffer = reader.fill_buf().await?;
            if buffer.is_empty() {
                if self.skipping || self.line.is_empty() {
                    self.skipping = false;
                    self.line.clear();
                    return Ok(Line::End);
                }
                return Ok(Line::Full(std::mem::take(&mut self.line)));
            }
            let (used, done) = match buffer.iter().position(|byte| *byte == b'\n') {
                Some(end) => (end + 1, true),
                None => (buffer.len(), false),
            };
            if self.skipping {
                reader.consume(used);
                if done {
                    self.skipping = false;
                }
                continue;
            }
            let content = if done { used - 1 } else { used };
            if self.line.len() + content > self.max {
                self.line = Vec::new();
                // The rest of this line, up to its break, is dropped by later reads.
                self.skipping = true;
                return Ok(Line::TooLong);
            }
            self.line.extend_from_slice(&buffer[..content]);
            reader.consume(used);
            if done {
                return Ok(Line::Full(std::mem::take(&mut self.line)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt, BufReader};

    use super::*;

    async fn all(input: &[u8], max: usize, capacity: usize) -> Vec<Line> {
        let mut reader = BufReader::with_capacity(capacity, input);
        let mut lines = Lines::new(max);
        let mut out = Vec::new();
        loop {
            let line = lines.next(&mut reader).await.unwrap();
            assert!(lines.line.len() <= max);
            let end = line == Line::End;
            out.push(line);
            if end {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn reads_lines_and_a_last_one_without_a_break() {
        let got = all(b"one\ntwo\n\nlast", 10, 2).await;
        assert_eq!(
            got,
            [
                Line::Full(b"one".to_vec()),
                Line::Full(b"two".to_vec()),
                Line::Full(Vec::new()),
                Line::Full(b"last".to_vec()),
                Line::End
            ]
        );
    }

    #[tokio::test]
    async fn a_line_at_the_cap_passes_and_one_past_it_is_skipped() {
        let got = all(b"12345\n123456789\nafter\n", 5, 3).await;
        assert_eq!(
            got,
            [
                Line::Full(b"12345".to_vec()),
                Line::TooLong,
                Line::Full(b"after".to_vec()),
                Line::End
            ]
        );
    }

    #[tokio::test]
    async fn an_endless_line_never_grows_past_the_cap() {
        // A process that writes 8 MiB with no line break, read with a 1 KiB cap: the
        // buffer stays under the cap while the rest is drained and dropped.
        let (mut write, read) = tokio::io::duplex(64 * 1024);
        let writer = tokio::spawn(async move {
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..128 {
                write.write_all(&chunk).await.unwrap();
            }
            write.write_all(b"\n{}\n").await.unwrap();
        });
        let mut reader = BufReader::new(read);
        let mut lines = Lines::new(1024);
        assert_eq!(lines.next(&mut reader).await.unwrap(), Line::TooLong);
        assert!(lines.line.capacity() <= 1024);
        assert_eq!(
            lines.next(&mut reader).await.unwrap(),
            Line::Full(b"{}".to_vec())
        );
        assert!(lines.line.capacity() <= 1024);
        assert_eq!(lines.next(&mut reader).await.unwrap(), Line::End);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn a_too_long_last_line_ends_without_a_part() {
        let got = all(b"ok\nxxxxxxxxxxxx", 4, 4).await;
        assert_eq!(got, [Line::Full(b"ok".to_vec()), Line::TooLong, Line::End]);
    }
}
