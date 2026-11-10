//! Which files a command changed in the project. git's list of changes is read before and
//! after; a file that is new in the list, or whose size or time moved, was touched.

use std::{collections::HashMap, path::Path, time::SystemTime};

use dusk_protocol::project::{Change, ChangeKind};

/// Each changed file with its kind, size and change time.
pub struct Snapshot {
    files: HashMap<String, (ChangeKind, Option<(u64, SystemTime)>)>,
}

pub async fn snapshot(dir: &Path) -> Snapshot {
    let changes = crate::project::git_changes(dir).await;
    let dir = dir.to_owned();
    let files = tokio::task::spawn_blocking(move || {
        changes
            .into_iter()
            .map(|change| {
                let stat = std::fs::symlink_metadata(dir.join(&change.path))
                    .ok()
                    .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
                (change.path, (change.kind, stat))
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    Snapshot { files }
}

impl Snapshot {
    /// The files that are new in `after`, or differ from this snapshot, and files that
    /// were changed before and are clean now.
    pub fn touched(&self, after: &Snapshot) -> Vec<Change> {
        let mut touched: Vec<Change> = after
            .files
            .iter()
            .filter(|(path, now)| self.files.get(*path) != Some(*now))
            .map(|(path, (kind, _))| Change {
                path: path.clone(),
                kind: *kind,
            })
            .collect();
        touched.sort_by(|a, b| a.path.cmp(&b.path));
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(files: &[(&str, ChangeKind, u64)]) -> Snapshot {
        let at = SystemTime::UNIX_EPOCH;
        Snapshot {
            files: files
                .iter()
                .map(|(path, kind, len)| ((*path).to_owned(), (*kind, Some((*len, at)))))
                .collect(),
        }
    }

    #[test]
    fn only_new_or_moved_files_count() {
        let before = snap(&[
            ("a", ChangeKind::Modified, 1),
            ("b", ChangeKind::Modified, 1),
        ]);
        let after = snap(&[
            ("a", ChangeKind::Modified, 1),
            ("b", ChangeKind::Modified, 2),
            ("c", ChangeKind::Untracked, 5),
        ]);
        let paths: Vec<_> = before.touched(&after).into_iter().map(|c| c.path).collect();
        assert_eq!(paths, ["b", "c"]);
    }

    #[tokio::test]
    async fn a_repository_shows_the_file_a_command_wrote() {
        let dir = std::env::temp_dir().join(format!("dusk-touched-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let init = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .arg("init")
            .output();
        if init.is_err() {
            return;
        }
        std::fs::write(dir.join("old.txt"), "x").unwrap();
        let before = snapshot(&dir).await;
        std::fs::write(dir.join("new.txt"), "y").unwrap();
        let touched = before.touched(&snapshot(&dir).await);
        let paths: Vec<_> = touched.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["new.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
