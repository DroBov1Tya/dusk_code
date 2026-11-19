//! Pictures in sources: telling a PNG or JPEG by its bytes, making a large one smaller
//! for the model, and keeping what was made so a pinned picture is not made again on
//! every step.

use std::{
    collections::VecDeque,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::SystemTime,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits, codecs::jpeg::JpegEncoder};

use crate::providers::Picture;

/// A picture past either of these is made smaller for the model.
pub(super) const MAX_PICTURE_EDGE: u32 = 1568;
const MAX_PICTURE_BYTES: usize = 4 * 1024 * 1024;
const JPEG_QUALITY: u8 = 85;
/// What a picture may take to decode; a larger one stays a file.
const MAX_DECODED_EDGE: u32 = 16_384;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn is_picture(bytes: &[u8]) -> bool {
    matches!(
        image::guess_format(bytes),
        Ok(ImageFormat::Png | ImageFormat::Jpeg)
    )
}

/// A PNG or JPEG as it is when small enough, else made smaller as a JPEG.
pub(crate) fn picture(bytes: &[u8]) -> Option<Picture> {
    let format = image::guess_format(bytes).ok()?;
    let mime_type = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return None,
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_EDGE);
    limits.max_image_height = Some(MAX_DECODED_EDGE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    if bytes.len() <= MAX_PICTURE_BYTES && image.width().max(image.height()) <= MAX_PICTURE_EDGE {
        return Some(Picture {
            mime_type,
            base64: STANDARD.encode(bytes),
        });
    }
    let smaller = image
        .thumbnail(MAX_PICTURE_EDGE, MAX_PICTURE_EDGE)
        .to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode_image(&smaller)
        .ok()?;
    Some(Picture {
        mime_type: "image/jpeg",
        base64: STANDARD.encode(jpeg),
    })
}

/// What tells one state of a picture file from another: its path, length and time of
/// change and, on Unix, its device, inode and status-change time. A copy that keeps the
/// time (`cp -p`, `touch -r`) or a rewrite within a coarse clock's tick keeps the length
/// and time; no program can set the status-change time, and a replacing rename brings
/// another inode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PictureKey {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    node: (u64, u64, i64, i64),
}

impl PictureKey {
    /// None when the file cannot be looked at, or the system keeps no time of change.
    pub(super) async fn of(path: &Path) -> Option<Self> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        #[cfg(unix)]
        let node = {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        };
        Some(Self {
            path: path.to_owned(),
            len: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            node,
        })
    }
}

/// The most pictures, and the most base64 bytes, kept made.
const MAX_KEPT: usize = 16;
const MAX_KEPT_BYTES: usize = 64 * 1024 * 1024;

/// Pictures as `picture` made them, the least recently used dropped first.
pub(super) struct Kept {
    entries: Mutex<VecDeque<(PictureKey, Picture)>>,
    max: usize,
    max_bytes: usize,
}

/// The pictures every chat's requests share.
pub(super) static KEPT: LazyLock<Kept> = LazyLock::new(|| Kept::new(MAX_KEPT, MAX_KEPT_BYTES));

impl Kept {
    pub(super) fn new(max: usize, max_bytes: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max,
            max_bytes,
        }
    }

    pub(super) fn get(&self, key: &PictureKey) -> Option<Picture> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let at = entries.iter().position(|(kept, _)| kept == key)?;
        let entry = entries.remove(at)?;
        let picture = entry.1.clone();
        entries.push_back(entry);
        Some(picture)
    }

    pub(super) fn put(&self, key: PictureKey, picture: Picture) {
        if picture.base64.len() > self.max_bytes {
            return;
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|(kept, _)| kept.path != key.path);
        entries.push_back((key, picture));
        let mut bytes: usize = entries.iter().map(|(_, p)| p.base64.len()).sum();
        while entries.len() > self.max || bytes > self.max_bytes {
            let Some((_, dropped)) = entries.pop_front() else {
                break;
            };
            bytes -= dropped.base64.len();
        }
    }
}
