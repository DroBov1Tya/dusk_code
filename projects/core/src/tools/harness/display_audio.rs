//! Deliberate, root-only sound cards for the user, from a file in the chat's folders: a
//! player with a play button in the chat. The file is checked by its first bytes, not its
//! name, and kept in the artifact store, so the card outlives the file.

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{Tool, ToolContext, ToolResult, schema, schema::parse_arguments};
use crate::util::fs::{printable, read_regular};
use crate::{
    BoxFuture,
    state::artifacts::{ArtifactStore, Card, MAX_AUDIO_BYTES},
};
use dusk_protocol::artifact::CardKind;

pub struct DisplayAudio {
    store: ArtifactStore,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// An MP3, WAV, OGG, FLAC or M4A file.
    path: String,
    #[schemars(length(max = 160))]
    title: Option<String>,
}

/// A sound found in a file's first bytes.
#[derive(Debug, PartialEq, Eq)]
struct Sound {
    mime_type: &'static str,
    /// Known only where the header says it, as a WAV's does.
    duration_ms: u64,
}

impl DisplayAudio {
    pub fn new(store: ArtifactStore) -> Self {
        Self { store }
    }

    async fn display(&self, arguments: Arguments, context: ToolContext) -> Result<ToolResult> {
        let title = match arguments.title {
            Some(title) => {
                ensure!(title.chars().count() <= 160, "audio card title is too long");
                Some(printable(&title))
            }
            None => None,
        };
        let path = context.resolve(&arguments.path, false).await?;
        let bytes =
            tokio::task::spawn_blocking(move || read_regular(&path, MAX_AUDIO_BYTES as u64))
                .await??;
        let Some(sound) = sniff(&bytes) else {
            anyhow::bail!("not a sound of a known kind: MP3, WAV, OGG, FLAC and M4A are taken");
        };
        let card = self.store.save_audio(
            Card {
                title,
                mime_type: sound.mime_type.into(),
                kind: CardKind::Audio,
                duration_ms: sound.duration_ms,
                ..Card::default()
            },
            &bytes,
        )?;
        Ok(ToolResult {
            content: format!(
                "Displayed audio card {} ({}, {} s).",
                card.id,
                card.mime_type,
                card.duration_ms / 1000
            ),
            is_error: false,
            exit_code: None,
            artifacts: vec![card],
        })
    }
}

impl Tool for DisplayAudio {
    fn name(&self) -> &'static str {
        "display_audio"
    }

    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Show the user a sound as a persistent card with a play button: an MP3, WAV, OGG, \
         FLAC or M4A file at path, up to 32 MB. Call deliberately for sound you made or \
         fetched, such as speech or music, not for every file. An optional title labels \
         the card. Only the root agent has this tool."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, _arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async { anyhow::bail!("audio tools require typed result transport") })
    }

    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move { self.display(parse_arguments(arguments)?, context).await })
    }
}

/// Keeps a sound some server made as a card: checked by its first bytes like a file.
pub fn save_sound(store: &ArtifactStore, bytes: &[u8], title: Option<String>) -> Result<Card> {
    let Some(sound) = sniff(bytes) else {
        anyhow::bail!("the speech server did not send a sound of a known kind");
    };
    store.save_audio(
        Card {
            title,
            mime_type: sound.mime_type.into(),
            kind: CardKind::Audio,
            duration_ms: sound.duration_ms,
            ..Card::default()
        },
        bytes,
    )
}

/// What the first bytes say a file is, or `None` when they are no sound this takes.
fn sniff(bytes: &[u8]) -> Option<Sound> {
    let sound = |mime_type, duration_ms| {
        Some(Sound {
            mime_type,
            duration_ms,
        })
    };
    match bytes {
        [b'I', b'D', b'3', ..] => sound("audio/mpeg", 0),
        [0xFF, second, ..] if second & 0xE0 == 0xE0 => sound("audio/mpeg", 0),
        [b'O', b'g', b'g', b'S', ..] => sound("audio/ogg", 0),
        [b'f', b'L', b'a', b'C', ..] => sound("audio/flac", 0),
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
        ] => sound("audio/wav", wav_duration_ms(bytes)),
        // An M4A or M4B brand; other MP4 files are video.
        [_, _, _, _, b'f', b't', b'y', b'p', brand @ ..]
            if brand.starts_with(b"M4A ") || brand.starts_with(b"M4B ") =>
        {
            sound("audio/mp4", 0)
        }
        _ => None,
    }
}

/// The length of a WAV from its `fmt ` and `data` chunks; zero when they do not say.
fn wav_duration_ms(bytes: &[u8]) -> u64 {
    let mut byte_rate = 0u64;
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = u64::from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]));
        match &header[..4] {
            b"fmt " => {
                let Some(rate) = bytes.get(at + 16..at + 20) else {
                    return 0;
                };
                byte_rate = u64::from(u32::from_le_bytes([rate[0], rate[1], rate[2], rate[3]]));
            }
            b"data" if byte_rate > 0 => return size.min(bytes.len() as u64) * 1000 / byte_rate,
            _ => {}
        }
        // Chunks are padded to an even size.
        let Some(next) = (at as u64).checked_add(8 + size + (size & 1)) else {
            return 0;
        };
        at = usize::try_from(next).unwrap_or(usize::MAX);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(seconds: u32) -> Vec<u8> {
        let rate = 16_000u32 * 2;
        let data = rate * seconds;
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 1, 0]);
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&[2, 0, 16, 0]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, data as usize));
        bytes
    }

    #[test]
    fn a_sound_is_known_by_its_first_bytes_not_its_name() {
        assert_eq!(sniff(b"ID3\x04rest").unwrap().mime_type, "audio/mpeg");
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90]).unwrap().mime_type, "audio/mpeg");
        assert_eq!(sniff(b"OggS\0").unwrap().mime_type, "audio/ogg");
        assert_eq!(sniff(b"fLaC\0").unwrap().mime_type, "audio/flac");
        assert_eq!(
            sniff(b"\0\0\0\x20ftypM4A \0").unwrap().mime_type,
            "audio/mp4"
        );
        // Video, pictures, scripts and short or empty files are not sounds.
        assert!(sniff(b"\0\0\0\x20ftypisom\0").is_none());
        assert!(sniff(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(sniff(b"#!/bin/sh\n").is_none());
        assert!(sniff(b"").is_none());
        assert!(sniff(&[0xFF]).is_none());
    }

    #[test]
    fn a_wav_says_how_long_it_is() {
        assert_eq!(sniff(&wav(6)).unwrap().duration_ms, 6000);
    }

    #[test]
    fn a_broken_wav_never_panics() {
        let whole = wav(1);
        for cut in 0..whole.len().min(80) {
            let _ = sniff(&whole[..cut]);
        }
        let mut huge = whole.clone();
        huge[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(sniff(&huge).is_some());
        // A chunk size that runs past the end ends the walk.
        let mut loops = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        loops.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(sniff(&loops).unwrap().duration_ms, 0);
    }
}
