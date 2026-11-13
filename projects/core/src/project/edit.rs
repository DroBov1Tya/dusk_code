//! A text file the user edits in the window: read with a stamp of what it was on disk,
//! and written back whole, only where it may be read and only while that stamp still holds.

use std::{
    fs::Metadata,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::project::{FileStamp, FileText, MAX_EDIT_BYTES};

use crate::util::{atomic, fs::read_regular};

pub async fn read(path: &Path, roots: &[PathBuf]) -> Result<FileText> {
    let path = super::within(path, roots).await?;
    tokio::task::spawn_blocking(move || {
        // Stamped before it is read: a change in between makes the stamp stale, so the
        // next write is refused rather than laid over text the user never saw.
        let stamp = stamp(&std::fs::symlink_metadata(&path).context("cannot read the file")?);
        let bytes = read_regular(&path, MAX_EDIT_BYTES)?;
        let text = String::from_utf8(bytes).ok().context("not a text file")?;
        Ok(FileText { text, stamp })
    })
    .await
    .context("cannot read the file")?
}

/// Replaces the file with `text`, or makes it in a folder that is there. Refused outside
/// `roots`, for anything but a regular file, and when `seen` is not what is on disk.
pub async fn write(
    path: &Path,
    text: String,
    seen: Option<FileStamp>,
    roots: &[PathBuf],
) -> Result<FileStamp> {
    ensure!(
        text.len() as u64 <= MAX_EDIT_BYTES,
        "the file is larger than {MAX_EDIT_BYTES} bytes"
    );
    let path = target(path, roots).await?;
    tokio::task::spawn_blocking(move || replace(&path, text.as_bytes(), seen))
        .await
        .context("cannot write the file")?
}

/// Where a write lands, resolved and inside `roots`: the file a name leads to, a link
/// followed, or for a name that is not there the same name in its resolved folder.
async fn target(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "the path must be absolute");
    // A link that leads nowhere exists and does not resolve, so it is refused below
    // instead of being written through.
    if tokio::fs::symlink_metadata(path).await.is_ok() {
        return super::within(path, roots).await;
    }
    let name = path.file_name().context("the path names no file")?;
    let folder = path.parent().context("the path names no file")?;
    Ok(super::within(folder, roots).await?.join(name))
}

fn replace(path: &Path, bytes: &[u8], seen: Option<FileStamp>) -> Result<FileStamp> {
    // `path` is resolved, so a link here was put in after the check; it is not followed.
    let on_disk = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(metadata.is_file(), "only a regular file can be written");
            Some(metadata)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("cannot read the file"),
    };
    if let Some(seen) = seen {
        ensure!(
            on_disk.as_ref().map(stamp) == Some(seen),
            "the file changed on disk since it was opened"
        );
    }
    atomic::write_keeping(path, bytes, on_disk.map(|metadata| metadata.permissions()))?;
    Ok(stamp(
        &std::fs::symlink_metadata(path).context("cannot read the file")?,
    ))
}

fn stamp(metadata: &Metadata) -> FileStamp {
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as i64);
    FileStamp {
        modified_ms,
        bytes: metadata.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    /// A project folder and a folder beside it that no root covers, both resolved.
    fn folders(name: &str) -> (PathBuf, PathBuf) {
        let base = temp_dir(name);
        std::fs::create_dir_all(base.join("project")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        let base = std::fs::canonicalize(base).unwrap();
        (base.join("project"), base.join("outside"))
    }

    #[tokio::test]
    async fn a_write_inside_a_project_answers_with_the_new_stamp() {
        let (project, _) = folders("edit-inside");
        let roots = vec![project.clone()];
        let file = project.join("notes.md");
        std::fs::write(&file, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let opened = read(&file, &roots).await.unwrap();
        assert_eq!(opened.text, "old");

        let saved = write(&file, "new text".into(), Some(opened.stamp), &roots)
            .await
            .unwrap();
        assert_eq!(saved.bytes, 8);
        assert_ne!(saved, opened.stamp);
        assert_eq!(read(&file, &roots).await.unwrap().stamp, saved);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new text");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o7777, 0o640);
        }

        let made = project.join("made.txt");
        let stamp = write(&made, "x".into(), None, &roots).await.unwrap();
        assert_eq!(stamp.bytes, 1);
        assert!(write(&project, "x".into(), None, &roots).await.is_err());
        let large = "x".repeat(MAX_EDIT_BYTES as usize + 1);
        assert!(write(&made, large, None, &roots).await.is_err());
        assert_eq!(std::fs::read_to_string(&made).unwrap(), "x");
    }

    #[tokio::test]
    async fn a_write_outside_every_root_is_refused() {
        let (project, outside) = folders("edit-outside");
        let roots = vec![project.clone()];
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "theirs").unwrap();

        let climbing = project.join("..").join("outside");
        let unresolved = project.join("sub").join("..").join("..").join("outside");
        for path in [
            secret.clone(),
            outside.join("new.txt"),
            climbing.join("secret.txt"),
            climbing.join("new.txt"),
            unresolved.join("new.txt"),
            project.join(".."),
            PathBuf::from("notes.md"),
        ] {
            let refused = write(&path, "mine".into(), None, &roots).await;
            assert!(refused.is_err(), "{}", path.display());
        }
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "theirs");
        assert!(!outside.join("new.txt").exists());
        assert!(read(&secret, &roots).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_that_leads_outside_is_refused() {
        use std::os::unix::fs::symlink;

        let (project, outside) = folders("edit-link");
        let roots = vec![project.clone()];
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "theirs").unwrap();
        symlink(&secret, project.join("file")).unwrap();
        symlink(&outside, project.join("folder")).unwrap();
        symlink(outside.join("absent.txt"), project.join("dangling")).unwrap();

        for path in [
            project.join("file"),
            project.join("folder").join("secret.txt"),
            project.join("folder").join("new.txt"),
            project.join("dangling"),
        ] {
            let refused = write(&path, "mine".into(), None, &roots).await;
            assert!(refused.is_err(), "{}", path.display());
        }
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "theirs");
        assert!(!outside.join("new.txt").exists());
        assert!(!outside.join("absent.txt").exists());

        // A link that stays inside is followed: the file it names is the one replaced.
        std::fs::write(project.join("real.txt"), "old").unwrap();
        symlink(project.join("real.txt"), project.join("alias")).unwrap();
        write(&project.join("alias"), "new".into(), None, &roots)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join("real.txt")).unwrap(),
            "new"
        );
        assert!(project.join("alias").is_symlink());
    }

    #[tokio::test]
    async fn a_file_changed_since_it_was_opened_is_left_as_it_is() {
        let (project, _) = folders("edit-stale");
        let roots = vec![project.clone()];
        let file = project.join("notes.md");
        std::fs::write(&file, "old").unwrap();
        let opened = read(&file, &roots).await.unwrap();
        std::fs::write(&file, "changed by another program").unwrap();

        let error = write(&file, "mine".into(), Some(opened.stamp), &roots)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("changed on disk"));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "changed by another program"
        );

        // A file that went away since is a change too.
        std::fs::remove_file(&file).unwrap();
        assert!(
            write(&file, "mine".into(), Some(opened.stamp), &roots)
                .await
                .is_err()
        );
        assert!(!file.exists());
    }
}
