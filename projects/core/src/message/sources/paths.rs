//! Picture paths in pasted text, as a terminal pastes a dropped file.

use std::path::PathBuf;

/// Words of `text` that name a PNG or JPEG by its full path, as a terminal pastes a
/// dropped file: quoted, with spaces escaped, or as a `file://` URL.
pub(super) fn picture_paths(text: &str) -> Vec<(std::ops::Range<usize>, PathBuf)> {
    let mut found = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(start, first)) = chars.peek() {
        if first.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        let mut end = start;
        if matches!(first, '\'' | '"') {
            chars.next();
            for (at, c) in chars.by_ref() {
                end = at + c.len_utf8();
                if c == first {
                    break;
                }
                word.push(c);
            }
        } else {
            while let Some((at, c)) = chars.next() {
                if c.is_whitespace() {
                    break;
                }
                end = at + c.len_utf8();
                match c {
                    '\\' => {
                        if let Some((at, escaped)) = chars.next() {
                            end = at + escaped.len_utf8();
                            word.push(escaped);
                        }
                    }
                    c => word.push(c),
                }
            }
        }
        // Punctuation after a bare path ends the sentence; a quoted path is exact.
        let mut trimmed = word.as_str();
        if !matches!(first, '\'' | '"') {
            trimmed = word.trim_end_matches([',', ';', ':', '!', '?', ')']);
            end -= word.len() - trimmed.len();
        }
        if let Some(path) = picture_path(trimmed) {
            found.push((start..end, path));
        }
    }
    found
}

fn picture_path(word: &str) -> Option<PathBuf> {
    let path = match word.strip_prefix("file://") {
        Some(url) => PathBuf::from(percent_decoded(url)?),
        None => match word.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
            None => PathBuf::from(word),
        },
    };
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    (path.is_absolute() && matches!(extension.as_str(), "png" | "jpg" | "jpeg")).then_some(path)
}

fn percent_decoded(text: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut rest = text.bytes();
    while let Some(byte) = rest.next() {
        match byte {
            b'%' => {
                let hex = [rest.next()?, rest.next()?];
                bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
            }
            byte => bytes.push(byte),
        }
    }
    String::from_utf8(bytes).ok()
}
