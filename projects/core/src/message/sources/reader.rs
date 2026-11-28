//! A source given out in parts for a client to read: what it holds by its first bytes,
//! and text cut at a character's edge so no part splits one.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::thread::{SOURCE_CHUNK, SourceChunk, SourceKind};

use super::picture::is_picture;
use crate::init::files::open_no_follow;

/// How much of the start tells text from a binary file.
const SNIFF: usize = 8 * 1024;

/// The part of the file at `path` from byte `offset`. The last path component is not
/// followed, so a source swapped for a link cannot point the read elsewhere.
pub(super) fn read_chunk(path: &Path, offset: u64) -> Result<SourceChunk> {
    let mut file = open_no_follow(OpenOptions::new().read(true), path)?;
    let metadata = file.metadata().context("cannot read the source")?;
    ensure!(metadata.is_file(), "the source is not a regular file");
    let bytes = metadata.len();
    let mut head = Vec::with_capacity(SNIFF);
    (&mut file)
        .take(SNIFF as u64)
        .read_to_end(&mut head)
        .context("cannot read the source")?;
    let kind = kind(&head, bytes <= SNIFF as u64);
    let mut chunk = SourceChunk {
        kind,
        text: String::new(),
        offset: offset.min(bytes),
        next: None,
        bytes,
        path: path.display().to_string(),
    };
    if kind != SourceKind::Text || offset >= bytes {
        return Ok(chunk);
    }
    let (text, start, end) = text_from(&mut file, offset)?;
    chunk.text = text;
    chunk.offset = start;
    chunk.next = (end < bytes).then_some(end);
    Ok(chunk)
}

/// Text when the start is UTF-8 without a NUL; `whole` says the start is the whole file,
/// so a character cut at its end is no cut but a broken file.
fn kind(head: &[u8], whole: bool) -> SourceKind {
    if is_picture(&head[..head.len().min(16)]) {
        return SourceKind::Picture;
    }
    if head.contains(&0) {
        return SourceKind::Binary;
    }
    match std::str::from_utf8(head) {
        Ok(_) => SourceKind::Text,
        Err(error) if error.error_len().is_none() && !whole => SourceKind::Text,
        Err(_) => SourceKind::Binary,
    }
}

/// Up to `SOURCE_CHUNK` bytes from `offset` as text, with where they start and end. An
/// offset inside a character moves to the next one; a character cut by the end of the
/// part is left for the next. Bytes past the sniffed start that are no UTF-8 are shown
/// as the replacement character rather than refused.
fn text_from(file: &mut File, offset: u64) -> Result<(String, u64, u64)> {
    file.seek(SeekFrom::Start(offset))
        .context("cannot read the source")?;
    let mut buffer = Vec::with_capacity(SOURCE_CHUNK as usize);
    file.take(SOURCE_CHUNK)
        .read_to_end(&mut buffer)
        .context("cannot read the source")?;
    let full = buffer.len() as u64 == SOURCE_CHUNK;
    let skip = buffer
        .iter()
        .take(3)
        .take_while(|byte| is_continuation(**byte))
        .count();
    let end = if full {
        char_end(&buffer)
    } else {
        buffer.len()
    };
    let end = end.max(skip);
    let text = String::from_utf8_lossy(&buffer[skip..end]).into_owned();
    Ok((text, offset + skip as u64, offset + end as u64))
}

fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

/// The length of `buffer` without a character its end cuts short.
fn char_end(buffer: &[u8]) -> usize {
    // A character is at most four bytes, so its lead byte is among the last four.
    for back in 1..=buffer.len().min(4) {
        let at = buffer.len() - back;
        let byte = buffer[at];
        if is_continuation(byte) {
            continue;
        }
        let width = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if width > back { at } else { buffer.len() };
    }
    buffer.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_character_is_left_for_the_next_part() {
        let text = "ab\u{1F600}";
        let bytes = text.as_bytes();
        assert_eq!(char_end(&bytes[..4]), 2);
        assert_eq!(char_end(bytes), bytes.len());
        assert_eq!(char_end(b"abc"), 3);
    }

    #[test]
    fn kinds_are_told_by_the_first_bytes() {
        assert_eq!(kind(b"hello", true), SourceKind::Text);
        assert_eq!(kind(b"%PDF-1.7\0\x01", true), SourceKind::Binary);
        assert_eq!(kind(b"\x89PNG\r\n\x1a\n", true), SourceKind::Picture);
        assert_eq!(kind(b"ok \xE2\x82", false), SourceKind::Text);
        assert_eq!(kind(b"ok \xE2\x82", true), SourceKind::Binary);
        assert_eq!(kind(b"bad \xFF here", false), SourceKind::Binary);
    }

    #[test]
    fn parts_join_to_the_whole_text() {
        let dir = crate::testing::temp_dir("source-reader-parts");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        // Three-byte characters, so a part's end falls inside one.
        let text = "\u{20AC}".repeat(SOURCE_CHUNK as usize / 2);
        std::fs::write(&path, &text).unwrap();
        let mut joined = String::new();
        let mut offset = 0;
        loop {
            let chunk = read_chunk(&path, offset).unwrap();
            assert_eq!(chunk.kind, SourceKind::Text);
            assert!(!chunk.text.contains('\u{FFFD}'));
            joined.push_str(&chunk.text);
            match chunk.next {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(joined, text);
        // An offset inside a character starts at the next one.
        let chunk = read_chunk(&path, 1).unwrap();
        assert_eq!(chunk.offset, 3);
        // Past the end there is nothing more.
        let chunk = read_chunk(&path, text.len() as u64 + 10).unwrap();
        assert!(chunk.text.is_empty() && chunk.next.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        let dir = crate::testing::temp_dir("source-reader-link");
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("secret");
        std::fs::write(&secret, "secret").unwrap();
        let link = dir.join("source.txt");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert!(read_chunk(&link, 0).is_err());
    }
}
