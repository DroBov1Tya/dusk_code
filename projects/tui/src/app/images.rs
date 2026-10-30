//! Bounded local thumbnails for the root transcript. No URL or message text enters here.

use std::{cell::RefCell, collections::VecDeque, io::Cursor, sync::Arc};

use dusk_core::artifacts::{ArtifactStore, ImageArtifact};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};

const CACHE_SIZE: usize = 32;
const THUMBNAIL_EDGE: u32 = 256;
const DECODE_LIMIT: u64 = 2 * 1024 * 1024;

type Thumbnail = (ImageArtifact, Option<Arc<RgbaImage>>);

#[derive(Default)]
pub struct Images {
    store: Option<ArtifactStore>,
    cache: RefCell<VecDeque<Thumbnail>>,
}

impl Images {
    pub fn new(store: ArtifactStore) -> Self {
        Self {
            store: Some(store),
            cache: RefCell::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.store.is_some()
    }

    /// Missing and damaged thumbnails are cached too, so redraws never retry I/O.
    pub fn thumbnail(&self, artifact: &ImageArtifact) -> Option<Arc<RgbaImage>> {
        let mut cache = self.cache.borrow_mut();
        if let Some(index) = cache.iter().position(|(known, _)| known == artifact) {
            let cached = cache.remove(index)?;
            let image = cached.1.clone();
            cache.push_back(cached);
            return image;
        }
        let image = self.read(artifact).map(Arc::new);
        if cache.len() == CACHE_SIZE {
            cache.pop_front();
        }
        cache.push_back((artifact.clone(), image.clone()));
        image
    }

    fn read(&self, artifact: &ImageArtifact) -> Option<RgbaImage> {
        let store = self.store.as_ref()?;
        // Validate the original as well: a stale thumbnail must not hide a missing or
        // corrupt durable artifact. Neither read can trigger a network request.
        store.read(artifact).ok()?;
        let bytes = store.read_thumbnail(artifact).ok()?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(THUMBNAIL_EDGE);
        limits.max_image_height = Some(THUMBNAIL_EDGE);
        limits.max_alloc = Some(DECODE_LIMIT);
        let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        reader.limits(limits);
        let image = reader.decode().ok()?.to_rgba8();
        (image.width() > 0 && image.height() > 0).then_some(image)
    }
}
