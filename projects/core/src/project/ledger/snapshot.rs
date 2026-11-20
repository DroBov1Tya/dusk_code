//! What a project folder holds, by size and change time. It is walked as `search_files`
//! walks it: ignore rules apply with or without git, hidden files count, `.git` does not,
//! links are not followed, and no file is opened.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use ignore::WalkBuilder;

/// A larger project is not watched: two walks of it per command would cost too much.
const MAX_FILES: usize = 50_000;
/// One walk may take this long; a slower disk gives up rather than holding a command.
const TIME_BUDGET: Duration = Duration::from_millis(300);

/// A file's size and change time, as the walk saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: SystemTime,
}

impl Stamp {
    /// The stamp of the file at `path` itself, a link not followed.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        Self::from_metadata(&meta)
    }

    /// `None` for anything but a regular file.
    pub fn from_metadata(meta: &std::fs::Metadata) -> Option<Self> {
        meta.is_file().then_some(Self {
            size: meta.len(),
            modified: meta.modified().ok()?,
        })
    }
}

/// Every regular file under the root, by its path relative to it with `/` between parts.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub files: HashMap<String, Stamp>,
}

/// Walks `root`, leaving out the folders in `skip` (the config folder, the thread's
/// workspace). `None` when the project is past the caps: a partial picture would show
/// files as deleted that were only not reached.
pub fn take(root: &Path, skip: &[PathBuf]) -> Option<Snapshot> {
    take_within(root, skip, MAX_FILES, TIME_BUDGET)
}

/// `take` with a time budget of the caller's: the file map walks for the model, where
/// no command waits on it.
pub fn take_for(root: &Path, skip: &[PathBuf], budget: Duration) -> Option<Snapshot> {
    take_within(root, skip, MAX_FILES, budget)
}

fn take_within(
    root: &Path,
    skip: &[PathBuf],
    max_files: usize,
    budget: Duration,
) -> Option<Snapshot> {
    let started = Instant::now();
    let skip = skip.to_vec();
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry.file_name() != ".git" && !skip.iter().any(|folder| entry.path() == folder)
        });
    let mut files = HashMap::new();
    for entry in walk.build() {
        if files.len() >= max_files || started.elapsed() >= budget {
            return None;
        }
        // An entry that vanished mid-walk is not there.
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(stamp) = entry.metadata().ok().and_then(|m| Stamp::from_metadata(&m)) else {
            continue;
        };
        let Some(relative) = entry.path().strip_prefix(root).ok().and_then(Path::to_str) else {
            continue;
        };
        files.insert(relative.replace('\\', "/"), stamp);
    }
    Some(Snapshot { files })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::temp_dir;

    fn project(name: &str) -> PathBuf {
        let dir = temp_dir(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn ignored_files_git_and_skipped_folders_are_left_out() {
        let dir = project("ledger-walk");
        fs::write(dir.join(".gitignore"), "build/\n*.log\n").unwrap();
        fs::write(dir.join("src/a.rs"), "a").unwrap();
        fs::write(dir.join(".env"), "KEY=1").unwrap();
        fs::write(dir.join("run.log"), "x").unwrap();
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::write(dir.join("build/out"), "x").unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git/HEAD"), "x").unwrap();
        fs::create_dir_all(dir.join("work")).unwrap();
        fs::write(dir.join("work/tmp"), "x").unwrap();
        let snapshot = take(&dir, &[dir.join("work")]).unwrap();
        let mut paths: Vec<_> = snapshot.files.keys().map(String::as_str).collect();
        paths.sort();
        assert_eq!(paths, [".env", ".gitignore", "src/a.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_project_is_not_followed() {
        let dir = project("ledger-link");
        let outside = temp_dir("ledger-link-outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("out")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), dir.join("file")).unwrap();
        let snapshot = take(&dir, &[]).unwrap();
        assert!(snapshot.files.is_empty(), "{:?}", snapshot.files.keys());
    }

    #[test]
    fn past_a_cap_the_snapshot_is_none() {
        let dir = project("ledger-cap");
        for n in 0..5 {
            fs::write(dir.join(format!("src/{n}")), "x").unwrap();
        }
        assert!(take_within(&dir, &[], 3, TIME_BUDGET).is_none());
        assert!(take_within(&dir, &[], 10, Duration::ZERO).is_none());
        assert_eq!(
            take_within(&dir, &[], 10, TIME_BUDGET).unwrap().files.len(),
            5
        );
    }
}
