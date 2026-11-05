//! One folder of a project at a time, as a file tree opens it: hidden and ignored
//! entries are left out, and each entry carries what git says changed in it.

use anyhow::{Context, Result};
use dusk_protocol::project::{Change, FileEntry, Project};
use ignore::WalkBuilder;

use super::{git, inside};

/// More than this many entries in one folder are not shown.
const MAX_ENTRIES: usize = 2000;

pub async fn list(project: &Project, relative: &str) -> Result<Vec<FileEntry>> {
    let folder = inside(project, relative).await?;
    let changes = git::info(project).await?.changes;
    let root = project.dir.clone();
    let mut entries = tokio::task::spawn_blocking(move || {
        WalkBuilder::new(&folder)
            .max_depth(Some(1))
            .follow_links(false)
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.depth() == 1)
            .take(MAX_ENTRIES)
            .filter_map(|entry| {
                let path = entry.path().strip_prefix(&root).ok()?;
                let path = path.to_str()?.replace('\\', "/");
                let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
                Some(FileEntry {
                    change: change_of(&changes, &path, is_dir),
                    path,
                    is_dir,
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .context("cannot read the folder")?;
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.path.cmp(&b.path)));
    Ok(entries)
}

/// A file's own change, or one of a folder git reports as a whole; a folder also shows
/// the first change found inside it.
fn change_of(
    changes: &[Change],
    path: &str,
    is_dir: bool,
) -> Option<dusk_protocol::project::ChangeKind> {
    changes
        .iter()
        .find(|change| {
            change.path == path
                || path.starts_with(&format!("{}/", change.path))
                || is_dir && change.path.starts_with(&format!("{path}/"))
        })
        .map(|change| change.kind)
}

#[cfg(test)]
mod tests {
    use dusk_protocol::project::ChangeKind;

    use super::*;

    #[test]
    fn folders_show_changes_inside() {
        let changes = [
            Change {
                path: "src/a.rs".into(),
                kind: ChangeKind::Modified,
            },
            Change {
                path: "new".into(),
                kind: ChangeKind::Untracked,
            },
        ];
        assert_eq!(change_of(&changes, "src", true), Some(ChangeKind::Modified));
        assert_eq!(
            change_of(&changes, "src/a.rs", false),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            change_of(&changes, "new/x.rs", false),
            Some(ChangeKind::Untracked)
        );
        assert_eq!(change_of(&changes, "srcx", true), None);
    }
}
