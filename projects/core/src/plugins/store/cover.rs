//! Covers for the cards of the Workshop: fetched only as a signed list named them,
//! checked by size, SHA-256 and their first bytes, and kept in memory by hash so
//! drawing a list again does not fetch them again.

use std::{collections::VecDeque, sync::LazyLock};

use anyhow::{Context, Result, ensure};
use parking_lot::Mutex;

use super::{
    fetch::{self, Via},
    index, source,
};
use crate::config::Mirror;

/// Covers kept, newest last; at most 512 KiB each.
const KEPT: usize = 64;

/// (SHA-256, bytes) of each cover kept.
type Kept = VecDeque<(String, Vec<u8>)>;

static CACHE: LazyLock<Mutex<Kept>> = LazyLock::new(Mutex::default);

/// The cover of `name` from `source` and its type.
pub async fn get(mirrors: &[Mirror], source: &str, name: &str) -> Result<(Vec<u8>, &'static str)> {
    let seen = source::cover(mirrors, source, name)?;
    let cover = &seen.cover;
    let cached = CACHE
        .lock()
        .iter()
        .find(|(sha256, _)| *sha256 == cover.sha256)
        .map(|(_, bytes)| bytes.clone());
    let bytes = match cached {
        Some(bytes) => bytes,
        None => {
            let address = fetch::beside(&seen.url, &cover.path)?;
            fetch::fetch_checked(&address, cover.size, &cover.sha256, Via::Direct)
                .await
                .context("cannot fetch the cover")?
        }
    };
    let mime = sniff(&bytes).context("the cover is not a PNG or WebP picture")?;
    ensure!(
        index::cover_mime(&cover.path) == Some(mime),
        "the cover's bytes do not match its name"
    );
    let mut cache = CACHE.lock();
    if !cache.iter().any(|(sha256, _)| *sha256 == cover.sha256) {
        if cache.len() == KEPT {
            cache.pop_front();
        }
        cache.push_back((cover.sha256.clone(), bytes.clone()));
    }
    Ok((bytes, mime))
}

/// The type of a picture by its first bytes, which a client trusts over a name.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        Some("image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}
