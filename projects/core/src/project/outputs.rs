//! What the agents leave behind: the files under the workspace and the pictures folder,
//! and a text preview of one. A preview is read only inside those folders or a saved
//! project, so a client cannot read the rest of the disk through it.

use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{MAX_PREVIEW_BYTES, OutputFile, OutputKind, OutputPreview};
use ignore::WalkBuilder;

/// More files than this are not listed.
const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 6;

pub async fn list(roots: Vec<PathBuf>, artifacts: PathBuf) -> Result<Vec<OutputFile>> {
    let mut files = tokio::task::spawn_blocking(move || {
        let mut seen = HashSet::new();
        roots
            .iter()
            .filter(|root| root.is_dir())
            .flat_map(|root| {
                WalkBuilder::new(root)
                    .max_depth(Some(MAX_DEPTH))
                    .follow_links(false)
                    .build()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
                    .map(|entry| entry.into_path())
            })
            .filter(|path| !is_artifact_thumbnail(path, &artifacts) && seen.insert(path.clone()))
            .filter_map(|path| file(&path, &artifacts))
            .take(MAX_FILES)
            .collect::<Vec<_>>()
    })
    .await
    .context("cannot list the outputs")?;
    files.sort_by_key(|file| std::cmp::Reverse(file.modified));
    Ok(files)
}

pub async fn read(path: &Path, roots: &[PathBuf]) -> Result<String> {
    let path = tokio::fs::canonicalize(path)
        .await
        .context("no such file")?;
    let mut allowed = false;
    for root in roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await {
            allowed |= path.starts_with(root);
        }
    }
    ensure!(
        allowed,
        "only files of the workspace or a project can be shown"
    );
    let metadata = tokio::fs::metadata(&path).await?;
    ensure!(metadata.is_file(), "not a file");
    if metadata.len() > MAX_PREVIEW_BYTES {
        bail!("the file is too large to preview");
    }
    let bytes = tokio::fs::read(&path).await?;
    String::from_utf8(bytes).context("not a text file")
}

/// Removes one file the outputs list, only inside `roots`; folders are never removed.
pub async fn delete(path: &Path, roots: &[PathBuf]) -> Result<()> {
    let path = tokio::fs::canonicalize(path)
        .await
        .context("no such file")?;
    let mut allowed = false;
    for root in roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await {
            allowed |= path.starts_with(&root) && path != root;
        }
    }
    ensure!(allowed, "only files of the workspace can be deleted here");
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    ensure!(metadata.is_file(), "only a file can be deleted here");
    tokio::fs::remove_file(&path)
        .await
        .context("cannot delete the file")
}

fn file(path: &Path, artifacts: &Path) -> Option<OutputFile> {
    let metadata = path.metadata().ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let kind = if path.extension().is_none() && is_extensionless_image(path) {
        OutputKind::Image
    } else {
        kind(path)
    };
    let preview = if kind == OutputKind::Image
        && path.file_name().is_some_and(|name| name == "image")
        && is_managed_artifact(path, artifacts)
    {
        artifact_thumbnail(path, artifacts)
    } else {
        OutputPreview::Original
    };
    Some(OutputFile {
        path: path.to_path_buf(),
        kind,
        preview,
        bytes: metadata.len(),
        modified,
    })
}

fn is_managed_artifact(path: &Path, artifacts: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent.parent() == Some(artifacts)
            && parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    })
}

fn is_artifact_thumbnail(path: &Path, artifacts: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "thumbnail.png")
        && is_managed_artifact(path, artifacts)
}

fn artifact_thumbnail(original: &Path, artifacts: &Path) -> OutputPreview {
    use crate::state::artifacts::{MAX_THUMBNAIL_BYTES, THUMBNAIL_EDGE};

    let Some(id) = original.parent().and_then(Path::file_name) else {
        return OutputPreview::Unavailable;
    };
    let Ok(root) = artifacts.canonicalize() else {
        return OutputPreview::Unavailable;
    };
    let path = root.join(id).join("thumbnail.png");
    // A managed preview must resolve to its exact slot, even if the artifact
    // directory or preview was replaced with a symlink.
    if !path.canonicalize().is_ok_and(|resolved| resolved == path) {
        return OutputPreview::Unavailable;
    }
    let Ok(mut file) =
        crate::util::fs::open_no_follow(std::fs::OpenOptions::new().read(true), &path)
    else {
        return OutputPreview::Unavailable;
    };
    let mut header = [0; 24];
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_THUMBNAIL_BYTES as u64)
        || file.read_exact(&mut header).is_err()
        || &header[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"
    {
        return OutputPreview::Unavailable;
    }
    let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
    let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
    if (1..=THUMBNAIL_EDGE).contains(&width) && (1..=THUMBNAIL_EDGE).contains(&height) {
        OutputPreview::Thumbnail(path)
    } else {
        OutputPreview::Unavailable
    }
}

fn is_extensionless_image(path: &Path) -> bool {
    let mut signature = [0; 12];
    let Ok(mut file) =
        crate::util::fs::open_no_follow(std::fs::OpenOptions::new().read(true), path)
    else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file())
        || file.read_exact(&mut signature).is_err()
    {
        return false;
    }
    matches!(
        image::guess_format(&signature),
        Ok(image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP)
    )
}

pub fn kind(path: &Path) -> OutputKind {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "md" | "txt" | "json" | "yaml" | "yml" | "toml" | "log" | "rs" | "py" | "js" | "ts"
        | "html" | "css" | "sh" | "xml" | "ini" => OutputKind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => OutputKind::Image,
        "pdf" | "docx" | "doc" | "odt" | "rtf" | "csv" | "tsv" | "xlsx" | "xls" | "pptx"
        | "epub" => OutputKind::Document,
        _ => OutputKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[tokio::test]
    async fn lists_original_images_without_duplicate_previews() {
        let root = temp_dir("output-images");
        let nested = root.join("images");
        let artifact = nested.join(uuid::Uuid::now_v7().to_string());
        std::fs::create_dir_all(&artifact).unwrap();
        let image = artifact.join("image");
        std::fs::write(&image, b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d").unwrap();
        let thumbnail = artifact.join("thumbnail.png");
        image::RgbaImage::new(16, 16).save(&thumbnail).unwrap();
        let standalone = nested.join("thumbnail.png");
        std::fs::write(&standalone, b"standalone output").unwrap();
        let text = nested.join("notes");
        std::fs::write(&text, b"plain text output").unwrap();
        let files = list(vec![root, nested.clone()], nested).await.unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|file| file.path == standalone));
        let original = files.iter().find(|file| file.path == image).unwrap();
        assert_eq!(
            original.preview,
            OutputPreview::Thumbnail(thumbnail.canonicalize().unwrap())
        );
        std::fs::remove_file(&thumbnail).unwrap();
        assert_eq!(
            artifact_thumbnail(&image, artifact.parent().unwrap()),
            OutputPreview::Unavailable
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&standalone, &thumbnail).unwrap();
            assert_eq!(
                artifact_thumbnail(&image, artifact.parent().unwrap()),
                OutputPreview::Unavailable
            );
        }
        assert!(
            files
                .iter()
                .any(|file| file.path == image && file.kind == OutputKind::Image)
        );
        assert!(
            files
                .iter()
                .any(|file| file.path == text && file.kind == OutputKind::Other)
        );
    }

    #[tokio::test]
    async fn previews_stay_inside_the_roots() {
        let root = temp_dir("outputs");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("report.md"), "# done").unwrap();
        let outside = temp_dir("outputs-outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "no").unwrap();
        let roots = vec![root.clone()];
        assert_eq!(
            read(&root.join("report.md"), &roots).await.unwrap(),
            "# done"
        );
        assert!(read(&outside.join("secret.txt"), &roots).await.is_err());
        assert!(
            read(
                &root
                    .join("../")
                    .join(outside.file_name().unwrap())
                    .join("secret.txt"),
                &roots
            )
            .await
            .is_err()
        );
        assert!(delete(&outside.join("secret.txt"), &roots).await.is_err());
        assert!(delete(&root, &roots).await.is_err());
        let listed = list(roots.clone(), root.join("artifacts")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, OutputKind::Text);
        delete(&root.join("report.md"), &roots).await.unwrap();
        assert!(
            list(roots, root.join("artifacts"))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
