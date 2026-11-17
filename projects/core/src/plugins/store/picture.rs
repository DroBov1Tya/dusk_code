//! The pictures of the Workshop, covers and screenshots: fetched only as a signed list
//! named them, checked by size, SHA-256 and their first bytes, and kept in memory by
//! hash so drawing a list again does not fetch them again.

use std::{collections::VecDeque, sync::LazyLock};

use anyhow::{Context, Result, anyhow, ensure};
use parking_lot::Mutex;

use super::{
    fetch::{self, Via},
    index::{self, Cover},
    source,
};
use crate::config::Mirror;

/// The bytes kept for all pictures together: 64 covers or 32 screenshots at most.
const KEPT_BYTES: usize = 32 * 1024 * 1024;

/// (SHA-256, bytes) of each picture kept, newest last.
type Kept = VecDeque<(String, Vec<u8>)>;

static CACHE: LazyLock<Mutex<Kept>> = LazyLock::new(Mutex::default);

/// The cover of `name` from `source` and its type.
pub async fn cover(
    mirrors: &[Mirror],
    source: &str,
    name: &str,
) -> Result<(Vec<u8>, &'static str)> {
    let seen = source::seen(mirrors, source, name)?;
    let cover = seen
        .cover
        .ok_or_else(|| anyhow!("{source} showed no cover for {name}; list the store again"))?;
    get(&seen.url, &cover).await.context("the cover")
}

/// The screenshot at `index` of `name` from `source` and its type.
pub async fn screenshot(
    mirrors: &[Mirror],
    source: &str,
    name: &str,
    index: u32,
) -> Result<(Vec<u8>, &'static str)> {
    let seen = source::seen(mirrors, source, name)?;
    let picture = source::screenshot(&seen, index)
        .ok_or_else(|| {
            anyhow!("{source} showed no screenshot {index} for {name}; list the store again")
        })?
        .picture();
    get(&seen.url, &picture).await.context("the screenshot")
}

/// `picture` beside the list at `list_url`, from the cache or fetched.
async fn get(list_url: &str, picture: &Cover) -> Result<(Vec<u8>, &'static str)> {
    let cached = CACHE
        .lock()
        .iter()
        .find(|(sha256, _)| *sha256 == picture.sha256)
        .map(|(_, bytes)| bytes.clone());
    let bytes = match cached {
        Some(bytes) => bytes,
        None => {
            let address = fetch::beside(list_url, &picture.path)?;
            fetch::fetch_checked(&address, picture.size, &picture.sha256, Via::Direct)
                .await
                .context("cannot fetch it")?
        }
    };
    let mime = checked_type(&picture.path, &bytes)?;
    keep(&picture.sha256, &bytes);
    Ok((bytes, mime))
}

/// The type of `bytes` by their first bytes, when it is the one `path` names.
pub fn checked_type(path: &str, bytes: &[u8]) -> Result<&'static str> {
    let mime = sniff(bytes).context("it is not a PNG or WebP picture")?;
    ensure!(
        index::cover_mime(path) == Some(mime),
        "its bytes do not match its name"
    );
    Ok(mime)
}

/// Keeps `bytes` under `sha256`, dropping the oldest until all fit in `KEPT_BYTES`.
fn keep(sha256: &str, bytes: &[u8]) {
    let mut cache = CACHE.lock();
    if cache.iter().any(|(kept, _)| kept == sha256) {
        return;
    }
    let mut total: usize = cache.iter().map(|(_, kept)| kept.len()).sum();
    while total + bytes.len() > KEPT_BYTES {
        let Some((_, dropped)) = cache.pop_front() else {
            break;
        };
        total -= dropped.len();
    }
    cache.push_back((sha256.to_owned(), bytes.to_vec()));
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
