//! App-owned image attachments. Message text and provider requests contain no image bytes.

use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::tools::read_regular;

pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_THUMBNAIL_BYTES: usize = 512 * 1024;

/// A durable card reference, carried separately from model-visible text. IDs are generated
/// locally; neither a URL nor a caller-supplied path is used as a storage filename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Reads only a local attachment. A missing/corrupt file is an unavailable card;
    /// callers must show a placeholder and must never refetch the source URL.
    pub fn read(&self, artifact: &ImageArtifact) -> Result<Vec<u8>> {
        let bytes = read_regular(&self.file(artifact, "image")?, MAX_IMAGE_BYTES as u64)?;
        ensure!(
            bytes.len() as u64 == artifact.byte_len,
            "image artifact is incomplete"
        );
        ensure!(
            digest(&bytes) == artifact.sha256,
            "image artifact is damaged"
        );
        Ok(bytes)
    }

    /// A small PNG suitable for terminal rendering. The original remains available
    /// through `read`, but a client need not decode a full-resolution image again.
    pub fn read_thumbnail(&self, artifact: &ImageArtifact) -> Result<Vec<u8>> {
        read_regular(
            &self.file(artifact, "thumbnail.png")?,
            MAX_THUMBNAIL_BYTES as u64,
        )
    }

    pub(crate) fn save(
        &self,
        mut artifact: ImageArtifact,
        bytes: &[u8],
        thumbnail: &[u8],
    ) -> Result<ImageArtifact> {
        ensure!(
            bytes.len() <= MAX_IMAGE_BYTES,
            "image exceeds storage limit"
        );
        ensure!(
            thumbnail.len() <= MAX_THUMBNAIL_BYTES,
            "thumbnail exceeds storage limit"
        );
        artifact.id = Uuid::now_v7().to_string();
        artifact.byte_len = bytes.len() as u64;
        artifact.sha256 = digest(bytes);
        fs::create_dir_all(&self.root).context("create image artifact store")?;
        let directory = self.root.join(&artifact.id);
        fs::create_dir(&directory).context("create image artifact directory")?;
        let saved = (|| {
            write_new(&directory.join("image"), bytes)?;
            write_new(&directory.join("thumbnail.png"), thumbnail)?;
            Ok(artifact)
        })();
        if saved.is_err() {
            let _ = fs::remove_dir_all(&directory);
        }
        saved
    }

    fn file(&self, artifact: &ImageArtifact, name: &str) -> Result<PathBuf> {
        let parsed = Uuid::parse_str(&artifact.id).context("invalid image artifact ID")?;
        ensure!(
            parsed.to_string() == artifact.id,
            "invalid image artifact ID"
        );
        Ok(self.root.join(&artifact.id).join(name))
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).context("create image artifact file")?;
    file.write_all(bytes).context("write image artifact file")?;
    file.sync_all().context("flush image artifact file")
}
