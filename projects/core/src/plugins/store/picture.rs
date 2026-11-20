//! The pictures of the Workshop, covers and screenshots: fetched only as a signed list
//! named them, checked by size, SHA-256, their first bytes and the pixels their header
//! declares, and kept in memory by hash so drawing a list again does not fetch them
//! again.

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

/// The most pixels a cover may declare: drawn at 1200x630, twice that is kept. A client
/// decodes without limits, so a small file declaring a huge picture would take gigabytes.
pub const COVER_PIXELS: (u32, u32) = (2400, 1260);
/// The most pixels a screenshot may declare: drawn at 1600x1000, twice that is kept.
pub const SCREENSHOT_PIXELS: (u32, u32) = (3200, 2000);

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
    get(&seen.url, &cover, COVER_PIXELS)
        .await
        .context("the cover")
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
    get(&seen.url, &picture, SCREENSHOT_PIXELS)
        .await
        .context("the screenshot")
}

/// `picture` beside the list at `list_url`, from the cache or fetched, checked before it
/// is kept or returned.
async fn get(list_url: &str, picture: &Cover, most: (u32, u32)) -> Result<(Vec<u8>, &'static str)> {
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
    check_pixels(&bytes, most)?;
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

/// Refuses a picture whose header cannot be read, declares more than `most` pixels, or
/// is an animated WebP, before anything decodes it (CWE-409).
pub fn check_pixels(bytes: &[u8], most: (u32, u32)) -> Result<()> {
    let (width, height) = pixels(bytes).context("its size cannot be read from its header")?;
    ensure!(
        (1..=most.0).contains(&width) && (1..=most.1).contains(&height),
        "it is {width}x{height} pixels, more than {}x{}",
        most.0,
        most.1
    );
    ensure!(!animated(bytes), "it is an animated picture");
    Ok(())
}

/// Width and height from a PNG `IHDR` or a WebP `VP8 `, `VP8L` or `VP8X` header, without
/// decoding; none when the header is not one of those.
pub fn pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    let at = |range: std::ops::Range<usize>| bytes.get(range);
    let little = |range: std::ops::Range<usize>| {
        at(range).map(|part| {
            part.iter()
                .rev()
                .fold(0u32, |sum, byte| (sum << 8) | u32::from(*byte))
        })
    };
    match sniff(bytes)? {
        "image/png" => {
            if at(12..16)? != b"IHDR" {
                return None;
            }
            let width = u32::from_be_bytes(at(16..20)?.try_into().ok()?);
            let height = u32::from_be_bytes(at(20..24)?.try_into().ok()?);
            Some((width, height))
        }
        _ => match at(12..16)? {
            b"VP8X" => Some((little(24..27)? + 1, little(27..30)? + 1)),
            b"VP8 " if at(23..26)? == b"\x9d\x01\x2a" => {
                Some((little(26..28)? & 0x3FFF, little(28..30)? & 0x3FFF))
            }
            b"VP8L" if *bytes.get(20)? == 0x2F => {
                let bits = little(21..25)?;
                Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
            }
            _ => None,
        },
    }
}

/// An extended WebP with its animation flag set: frames a client would decode one by one.
fn animated(bytes: &[u8]) -> bool {
    const ANIMATION: u8 = 0x02;
    bytes.get(12..16) == Some(b"VP8X".as_slice())
        && bytes.get(20).is_some_and(|flags| flags & ANIMATION != 0)
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
