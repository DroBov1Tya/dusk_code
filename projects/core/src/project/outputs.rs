//! What the agents leave behind: the files under the workspace and the pictures folder,
//! and a text preview of one. A preview is read only inside those folders or a saved
//! project, so a client cannot read the rest of the disk through it.

use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{MAX_PREVIEW_BYTES, OutputFile, OutputKind};
use ignore::WalkBuilder;

/// More files than this are not listed.
const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 6;

pub async fn list(roots: Vec<PathBuf>) -> Result<Vec<OutputFile>> {
    let mut files = tokio::task::spawn_blocking(move || {
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
                    .filter_map(|entry| file(entry.path()))
            })
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

fn file(path: &Path) -> Option<OutputFile> {
    let metadata = path.metadata().ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some(OutputFile {
        path: path.to_path_buf(),
        kind: kind(path),
        bytes: metadata.len(),
        modified,
    })
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
        let listed = list(roots.clone()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, OutputKind::Text);
        delete(&root.join("report.md"), &roots).await.unwrap();
        assert!(list(roots).await.unwrap().is_empty());
    }
}
