//! The pictures of screen widgets. Each is decoded off the window's thread; while one is
//! decoded, only the newest that arrives waits, so a fast plugin never builds a queue.

use std::{io::Cursor, sync::Arc};

use gpui_kit::*;

use super::Slot;
use crate::workspace::Workspace;

/// The widest a picture is kept, in pixels: a dock at twice its points.
const SHOWN_WIDTH: u32 = 800;
/// The largest picture decoded at all; a JPEG that claims more is refused before its
/// pixels are made.
const MAX_SIDE: u32 = 8192;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

impl Workspace {
    pub(super) fn take_frame(&mut self, slot: Slot, jpeg: Vec<u8>, cx: &mut Context<Self>) {
        let plugins = &mut self.plugins;
        if plugins.decoding.contains(&slot) {
            plugins.waiting.insert(slot, jpeg);
            return;
        }
        plugins.decoding.insert(slot.clone());
        let task = cx.background_spawn(async move { decode(&jpeg) });
        cx.spawn(async move |ws, cx| {
            let picture = task.await;
            let _ = ws.update(cx, |ws, cx| ws.frame_decoded(slot, picture, cx));
        })
        .detach();
    }

    fn frame_decoded(
        &mut self,
        slot: Slot,
        picture: anyhow::Result<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let plugins = &mut self.plugins;
        plugins.decoding.remove(&slot);
        let open = plugins.is_open(&slot.0, self.modal.as_ref());
        match picture {
            Ok(picture) if open => {
                if let Some(old) = plugins.frames.insert(slot.clone(), picture) {
                    plugins.retired.push(old);
                }
                cx.notify();
            }
            Ok(_) => {}
            Err(error) => tracing::debug!("plugin {} frame: {error:#}", slot.0.plugin),
        }
        if let Some(next) = self.plugins.waiting.remove(&slot)
            && open
        {
            self.take_frame(slot, next, cx);
        }
    }
}

/// A JPEG as the picture the window draws: no wider than a dock needs, its pixels in the
/// BGRA order the window wants.
fn decode(jpeg: &[u8]) -> anyhow::Result<Arc<RenderImage>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(jpeg), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut full = reader.decode()?;
    if full.width() > SHOWN_WIDTH {
        full = full.resize(SHOWN_WIDTH, u32::MAX, image::imageops::FilterType::Triangle);
    }
    let mut pixels = full.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the window's prelude brings a `test` of its own.
    use super::decode;

    #[test]
    fn a_frame_that_is_no_jpeg_is_refused_without_a_panic() {
        assert!(decode(b"").is_err());
        assert!(decode(b"\xff\xd8\xff\xe0 not really").is_err());
        assert!(decode(&[0u8; 4096]).is_err());
    }
}
