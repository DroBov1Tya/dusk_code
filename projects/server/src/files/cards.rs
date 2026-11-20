//! The pictures and sounds of a chat, which the browser shows by address. They are read
//! from the artifact folder by ID, as the GUI does; a name that is not an ID reads nothing.

use dusk_core::client::ArtifactStore;
use std::path::Path;

/// The bytes of the card `id` in the artifact folder, `part` being `image`, `audio` or
/// `thumb`, and what they are. The core reads it, by the rule it keeps for every client.
pub fn read(folder: &Path, id: &str, part: &str) -> Option<(Vec<u8>, &'static str)> {
    let bytes = ArtifactStore::new(folder.to_owned())
        .read_part(id, part)
        .ok()?;
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
    fn a_card_is_named_by_what_its_bytes_are() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), "audio/wav");
        assert_eq!(sniff(b"<html>"), "application/octet-stream");
    }
}
