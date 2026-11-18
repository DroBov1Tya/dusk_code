//! Texts of project files as a thread last saw them, by `read_file` or as a change the
//! ledger recorded, so the next change of the file has a "before" to diff against. Only
//! in memory, for this run of the core, and capped in bytes.
//!
//! An entry is keyed by the file's absolute path and holds the stamp (size, change time)
//! the file had when the text was taken. The text counts as the file's "before" only
//! when the walk before a command finds the same stamp: that is the one check a walk can
//! make without reading the file.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use parking_lot::Mutex;

use super::Stamp;

/// A larger text is not kept; it would not get a diff under the diff caps anyway.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// All texts together; the least recently seen go first.
const MAX_BYTES: usize = 8 * 1024 * 1024;

struct Entry {
    stamp: Stamp,
    text: Arc<str>,
    /// When it was last put in or taken out, for eviction.
    used: u64,
}

#[derive(Default)]
struct Seen {
    entries: HashMap<PathBuf, Entry>,
    bytes: usize,
    clock: u64,
}

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Mutex::default);

/// Keeps `text` as the contents of `path` while it has `stamp`. The caller decides
/// whether the file may be kept at all: see [`super::may_keep_text`].
pub fn remember(path: &Path, stamp: Stamp, text: &str) {
    SEEN.lock().remember(path, stamp, text, MAX_BYTES);
}

/// The text of `path` when it was seen with exactly `stamp`.
pub fn recall(path: &Path, stamp: Stamp) -> Option<Arc<str>> {
    SEEN.lock().recall(path, stamp)
}

pub fn forget(path: &Path) {
    SEEN.lock().forget(path);
}

impl Seen {
    fn remember(&mut self, path: &Path, stamp: Stamp, text: &str, max_bytes: usize) {
        if text.len() > MAX_TEXT_BYTES {
            self.forget(path);
            return;
        }
        self.clock += 1;
        let entry = Entry {
            stamp,
            text: Arc::from(text),
            used: self.clock,
        };
        self.bytes += text.len();
        if let Some(old) = self.entries.insert(path.to_owned(), entry) {
            self.bytes -= old.text.len();
        }
        while self.bytes > max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.forget(&oldest);
        }
    }

    fn recall(&mut self, path: &Path, stamp: Stamp) -> Option<Arc<str>> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.get_mut(path)?;
        if entry.stamp != stamp {
            return None;
        }
        entry.used = clock;
        Some(Arc::clone(&entry.text))
    }

    fn forget(&mut self, path: &Path) {
        if let Some(gone) = self.entries.remove(path) {
            self.bytes -= gone.text.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn stamp(size: u64) -> Stamp {
        Stamp {
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(size),
        }
    }

    #[test]
    fn a_text_is_recalled_only_with_its_stamp() {
        let mut seen = Seen::default();
        let path = Path::new("/a");
        seen.remember(path, stamp(1), "one", MAX_BYTES);
        assert_eq!(seen.recall(path, stamp(1)).as_deref(), Some("one"));
        assert_eq!(seen.recall(path, stamp(2)), None);
        seen.remember(path, stamp(2), "two", MAX_BYTES);
        assert_eq!(seen.recall(path, stamp(1)), None);
        seen.forget(path);
        assert_eq!(seen.recall(path, stamp(2)), None);
        seen.remember(path, stamp(3), &"x".repeat(MAX_TEXT_BYTES + 1), MAX_BYTES);
        assert_eq!(seen.recall(path, stamp(3)), None);
        assert_eq!(seen.bytes, 0);
    }

    #[test]
    fn the_least_recently_seen_go_first_past_the_cap() {
        let mut seen = Seen::default();
        let path = |n: u64| PathBuf::from(format!("/{n}"));
        for n in 0..3 {
            seen.remember(&path(n), stamp(n), "1234", 10);
        }
        assert!(seen.bytes <= 10);
        assert_eq!(seen.recall(&path(0), stamp(0)), None, "the oldest went");
        assert!(seen.recall(&path(1), stamp(1)).is_some());
        seen.remember(&path(3), stamp(3), "1234", 10);
        assert!(
            seen.recall(&path(1), stamp(1)).is_some(),
            "recalled, so kept"
        );
        assert_eq!(seen.recall(&path(2), stamp(2)), None);
    }
}
