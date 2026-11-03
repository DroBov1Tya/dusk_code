//! Paths typed in a message: the entries of the folder a path points into that start
//! with what is typed after its last `/`. A folder may hold many thousands of entries,
//! so it is read as a stream and reading stops after `MAX_SCANNED` entries or
//! `MAX_FOUND` matches; the answer then says it is partial.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Names alone are cheap to read; only matches are looked at further.
const MAX_SCANNED: usize = 50_000;
const MAX_FOUND: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEntry {
    pub name: String,
    pub directory: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathCompletion {
    /// Matching entries, folders first, then by name.
    pub entries: Vec<PathEntry>,
    /// Reading stopped early: there may be more.
    pub partial: bool,
}

/// Whether a word of a message reads as a path to complete: `/`, `~/`, `./` or `../`.
pub fn is_path(word: &str) -> bool {
    ["/", "~/", "./", "../"]
        .iter()
        .any(|start| word.starts_with(start))
}

/// The entries `word` may go on with. `cwd` resolves relative paths; `~` is HOME.
/// A folder that cannot be read has none.
pub fn paths(word: &str, cwd: &Path) -> PathCompletion {
    let (folder, typed) = match word.rfind('/') {
        Some(at) => (&word[..=at], &word[at + 1..]),
        None => return PathCompletion::default(),
    };
    let Some(folder) = resolve(folder, cwd) else {
        return PathCompletion::default();
    };
    let Ok(listing) = fs::read_dir(folder) else {
        return PathCompletion::default();
    };
    let hidden = typed.starts_with('.');
    let mut completion = PathCompletion::default();
    for (scanned, entry) in listing.flatten().enumerate() {
        if scanned >= MAX_SCANNED || completion.entries.len() >= MAX_FOUND {
            completion.partial = true;
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(typed) || (name.starts_with('.') && !hidden) {
            continue;
        }
        // A link to a folder completes as a folder.
        let directory = fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
        completion.entries.push(PathEntry { name, directory });
    }
    completion
        .entries
        .sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
    completion
}

fn resolve(folder: &str, cwd: &Path) -> Option<PathBuf> {
    if let Some(rest) = folder.strip_prefix("~/") {
        return std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest));
    }
    Some(cwd.join(folder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn a_large_folder_is_read_in_part() {
        let root = temp_dir("complete");
        fs::create_dir_all(root.join("sub")).unwrap();
        for index in 0..MAX_FOUND + 10 {
            fs::write(root.join(format!("file{index}")), "").unwrap();
        }
        let word = format!("{}/", root.display());
        let all = paths(&word, Path::new("/"));
        assert!(all.partial);
        assert!(all.entries.len() <= MAX_FOUND);

        let sub = paths(&format!("{word}su"), Path::new("/"));
        assert_eq!(
            sub.entries,
            [PathEntry {
                name: "sub".into(),
                directory: true
            }]
        );
        assert!(!sub.partial);
    }
}
