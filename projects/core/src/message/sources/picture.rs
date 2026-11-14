//! Pictures in sources: telling a PNG or JPEG by its bytes, and making a large one
//! smaller for the model.

use std::io::Cursor;

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
