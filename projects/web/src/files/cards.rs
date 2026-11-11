//! The pictures and sounds of a chat, which the browser shows by address. They are read
//! from the artifact folder by ID, as the GUI does; a name that is not an ID reads nothing.

use std::path::{Path, PathBuf};

/// The largest card served; the core keeps its own cap when it saves one.
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// The file of the card `id`: `image`, `audio` or its small `thumb`nail. An ID is hex
/// digits and dashes only, so it cannot name a path outside the folder.
pub fn locate(folder: &Path, id: &str, part: &str) -> Option<PathBuf> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    let file = match part {
        "image" => "image",
        "audio" => "audio",
        "thumb" => "thumbnail.png",
        _ => return None,
    };
    valid.then(|| folder.join(id).join(file))
}

/// The bytes of a regular file under the cap and what they are.
pub async fn read(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let meta = tokio::fs::symlink_metadata(path).await.ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    let kind = sniff(&bytes);
    Some((bytes, kind))
}

pub fn sniff(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, 0xff, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'A',
            b'V',
            b'E',
            ..,
        ] => "audio/wav",
        [b'O', b'g', b'g', b'S', ..] => "audio/ogg",
        [b'f', b'L', b'a', b'C', ..] => "audio/flac",
        [b'I', b'D', b'3', ..] | [0xff, 0xfb, ..] => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_id_of_hex_and_dashes_names_a_card() {
        let folder = Path::new("/cards");
        assert!(locate(folder, "0a1b-2c", "image").is_some());
        assert!(locate(folder, "../etc", "image").is_none());
        assert!(locate(folder, "a/b", "image").is_none());
        assert!(locate(folder, "", "image").is_none());
        assert!(locate(folder, "ab", "passwd").is_none());
    }

    #[test]
    fn a_card_is_named_by_what_its_bytes_are() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), "audio/wav");
        assert_eq!(sniff(b"<html>"), "application/octet-stream");
    }
}
