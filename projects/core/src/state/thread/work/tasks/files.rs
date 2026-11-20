//! The paths only one task may change. The store checks their shape and nothing on disk:
//! whoever calls it has resolved links inside the project already.

use super::{
    error::{PlanError, shown},
    plan::MAX_FILES,
};

/// The longest entry, in bytes.
const MAX_ENTRY: usize = 512;

/// At most 16 entries, each relative with `/` between its parts; a folder ends in `/`.
/// There is no way to hold another shape, so nothing the store reads back needs checking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFiles(Vec<String>);

impl TaskFiles {
    pub fn new<Entry: AsRef<str>>(
        entries: impl IntoIterator<Item = Entry>,
    ) -> Result<Self, PlanError> {
        let mut files: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry.as_ref();
            check(entry)?;
            if files.iter().any(|known| known == entry) {
                continue;
            }
            if files.len() == MAX_FILES {
                return Err(PlanError::TooMany {
                    what: "files in a task",
                    max: MAX_FILES,
                });
            }
            files.push(entry.to_owned());
        }
        Ok(Self(files))
    }

    pub fn entries(&self) -> &[String] {
        &self.0
    }

    /// The first entry of each that overlap, if any do.
    pub fn overlap<'a>(&'a self, other: &'a Self) -> Option<(&'a str, &'a str)> {
        self.0.iter().find_map(|mine| {
            let theirs = other.0.iter().find(|theirs| overlaps(mine, theirs))?;
            Some((mine.as_str(), theirs.as_str()))
        })
    }
}

/// Two entries overlap when they name one path or one is a folder above the other. The
/// claim asks the same in SQL (`claims::OVERLAP`); the two must agree.
pub fn overlaps(one: &str, other: &str) -> bool {
    let above = |folder: &str, path: &str| folder.ends_with('/') && path.starts_with(folder);
    one.trim_end_matches('/') == other.trim_end_matches('/')
        || above(one, other)
        || above(other, one)
}

fn check(entry: &str) -> Result<(), PlanError> {
    let bad = |why| {
        Err(PlanError::BadFile {
            entry: shown(entry),
            why,
        })
    };
    if entry.is_empty() {
        return bad("a file entry is empty");
    }
    if entry.len() > MAX_ENTRY {
        return bad("the path is too long");
    }
    if entry.chars().any(char::is_control) {
        return bad("the path has a control character");
    }
    if entry.contains('\\') {
        return bad("write the path with / between its parts");
    }
    if entry.contains(['*', '?']) {
        return bad("name files and folders, not patterns");
    }
    if entry.starts_with('/') {
        return bad("the path must be relative to the project");
    }
    let path = entry.strip_suffix('/').unwrap_or(entry);
    for (index, part) in path.split('/').enumerate() {
        match part {
            "" => return bad("the path has an empty part"),
            "." | ".." => return bad("the path must stay inside the project"),
            // A drive, as `C:`: absolute on Windows.
            _ if index == 0 && part.len() == 2 && part.ends_with(':') => {
                return bad("the path must be relative to the project");
            }
            _ => {}
        }
    }
    Ok(())
}
