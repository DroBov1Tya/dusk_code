//! What changed between two snapshots of a project: each file created, modified or
//! deleted, its contents hashed when small, and a diff when the text before is known.

use std::{path::Path, sync::Arc, time::Duration};

use dusk_protocol::{project::EditKind, text::sha256_hex};
use similar::TextDiff;

use super::{Snapshot, Stamp, may_keep_text, seen};
use crate::init::files::read_regular;

/// More changes than this in one call are not recorded: such a call rewrote a tree, and
/// rows for it would bury every other change.
const MAX_EDITS: usize = 1000;
/// Files read after one call, to hash and diff them; the rest are recorded bare.
const MAX_READS: usize = 64;
/// A larger file is recorded bare, as `read_file` would not read it either.
const MAX_READ_BYTES: u64 = 1024 * 1024;
const MAX_DIFF_LINES: usize = 400;
const MAX_DIFF_BYTES: usize = 16 * 1024;
/// A diff that takes longer is approximated, never waited for.
const DIFF_TIMEOUT: Duration = Duration::from_millis(50);

/// One file one call changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Relative to the project, with `/` between parts.
    pub path: String,
    pub kind: EditKind,
    pub size_before: Option<u64>,
    pub size_after: Option<u64>,
    pub hash_after: Option<String>,
    pub diff: Option<Diff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub text: String,
    /// The text hit the caps and ends with a line saying so.
    pub cut: bool,
}

/// The changes from `before` to `after` under `root`, sorted by path; `None` when there
/// are too many to record. Reads the changed files, so it blocks.
pub fn compare(root: &Path, before: &Snapshot, after: &Snapshot) -> Option<Vec<Edit>> {
    let mut changed: Vec<(&str, Option<Stamp>, Option<Stamp>)> = after
        .files
        .iter()
        .filter(|(path, stamp)| before.files.get(*path) != Some(*stamp))
        .map(|(path, stamp)| (path.as_str(), before.files.get(path).copied(), Some(*stamp)))
        .chain(
            before
                .files
                .iter()
                .filter(|(path, _)| !after.files.contains_key(*path))
                .map(|(path, stamp)| (path.as_str(), Some(*stamp), None)),
        )
        .collect();
    if changed.len() > MAX_EDITS {
        return None;
    }
    changed.sort_by_key(|(path, ..)| *path);
    let mut reads = 0;
    Some(
        changed
            .into_iter()
            .filter_map(|(path, before, after)| edit(root, path, before, after, &mut reads))
            .collect(),
    )
}

/// `None` when nothing changed after all: the file was written with the same text.
fn edit(
    root: &Path,
    path: &str,
    before: Option<Stamp>,
    after: Option<Stamp>,
    reads: &mut usize,
) -> Option<Edit> {
    let kind = match (before, after) {
        (None, Some(_)) => EditKind::Created,
        (Some(_), Some(_)) => EditKind::Modified,
        (Some(_), None) => EditKind::Deleted,
        (None, None) => return None,
    };
    let absolute = root.join(path);
    let keep = may_keep_text(path);
    let old_text: Option<Arc<str>> = match before {
        None => Some(Arc::from("")),
        Some(stamp) if keep => seen::recall(&absolute, stamp),
        Some(_) => None,
    };
    let bytes = after
        .filter(|stamp| stamp.size <= MAX_READ_BYTES && *reads < MAX_READS)
        .and_then(|stamp| {
            *reads += 1;
            read_after(root, &absolute, stamp)
        });
    let hash_after = bytes.as_deref().map(sha256_hex);
    let new_text: Option<String> = match after {
        None => Some(String::new()),
        Some(_) => bytes
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|text| text.len() <= seen::MAX_TEXT_BYTES),
    };
    match (after, &new_text) {
        (Some(stamp), Some(text)) if keep => seen::remember(&absolute, stamp, text),
        _ => seen::forget(&absolute),
    }
    let diff = match (keep, old_text, new_text) {
        (true, Some(old), Some(new)) => {
            if kind == EditKind::Modified && *old == *new {
                return None;
            }
            Some(unified(&old, &new))
        }
        _ => None,
    };
    Some(Edit {
        path: path.to_owned(),
        kind,
        size_before: before.map(|stamp| stamp.size),
        size_after: after.map(|stamp| stamp.size),
        hash_after,
        diff,
    })
}

/// The file's bytes, if it is still what the walk saw and still inside the project.
fn read_after(root: &Path, absolute: &Path, stamp: Stamp) -> Option<Vec<u8>> {
    // A folder swapped for a link since the walk must not lead the read outside.
    let resolved = absolute.canonicalize().ok()?;
    if resolved != absolute || !resolved.starts_with(root) {
        return None;
    }
    if Stamp::of(absolute) != Some(stamp) {
        return None;
    }
    read_regular(absolute, MAX_READ_BYTES).ok()
}

/// A unified diff without file headers, the row names the file, cut to the caps.
fn unified(old: &str, new: &str) -> Diff {
    let text = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .to_string();
    cap(&text)
}

fn cap(text: &str) -> Diff {
    let total = text.lines().count();
    let mut kept = String::new();
    let mut lines = 0;
    for line in text.lines() {
        if lines == MAX_DIFF_LINES || kept.len() + line.len() + 1 > MAX_DIFF_BYTES {
            break;
        }
        kept.push_str(line);
        kept.push('\n');
        lines += 1;
    }
    if lines == total {
        return Diff {
            text: kept,
            cut: false,
        };
    }
    kept.push_str(&format!("[diff cut: {} more lines]\n", total - lines));
    Diff {
        text: kept,
        cut: true,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{project::ledger::snapshot, testing::temp_dir};

    fn project(name: &str) -> std::path::PathBuf {
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn take(dir: &Path) -> Snapshot {
        snapshot::take(dir, &[]).unwrap()
    }

    /// Writes `text` so the change time moves even on a coarse clock.
    fn rewrite(path: &Path, text: &str) {
        let before = Stamp::of(path);
        fs::write(path, text).unwrap();
        let file = fs::File::options().write(true).open(path).unwrap();
        let later = before.map_or(std::time::SystemTime::now(), |stamp| {
            stamp.modified + Duration::from_secs(2)
        });
        file.set_modified(later).unwrap();
    }

    #[test]
    fn created_modified_and_deleted_files_are_found_without_git() {
        let dir = project("ledger-compare");
        fs::write(dir.join("kept"), "same\n").unwrap();
        fs::write(dir.join("changed"), "one\n").unwrap();
        fs::write(dir.join("gone"), "bye\n").unwrap();
        let before = take(&dir);
        fs::write(dir.join("new"), "hello\n").unwrap();
        rewrite(&dir.join("changed"), "one\ntwo\n");
        fs::remove_file(dir.join("gone")).unwrap();
        let edits = compare(&dir, &before, &take(&dir)).unwrap();
        let kinds: Vec<_> = edits.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("changed", EditKind::Modified),
                ("gone", EditKind::Deleted),
                ("new", EditKind::Created),
            ]
        );
        let new = &edits[2];
        assert_eq!((new.size_before, new.size_after), (None, Some(6)));
        assert_eq!(new.hash_after.as_deref().map(str::len), Some(64));
        assert_eq!(new.diff.as_ref().unwrap().text, "@@ -0,0 +1 @@\n+hello\n");
        // Nothing was read before, so the old text of the others is unknown.
        assert_eq!(edits[0].diff, None);
        assert_eq!(edits[1].diff, None);
        assert_eq!(edits[1].hash_after, None);
    }

    #[test]
    fn a_text_seen_before_gives_a_diff_and_a_secret_never_does() {
        let dir = project("ledger-diff");
        fs::create_dir_all(dir.join(".ssh")).unwrap();
        for (name, text) in [
            ("a.txt", "one\n"),
            (".env.local", "KEY=old\n"),
            (".ssh/config", "Host a\n"),
        ] {
            fs::write(dir.join(name), text).unwrap();
            seen::remember(&dir.join(name), Stamp::of(&dir.join(name)).unwrap(), text);
        }
        let before = take(&dir);
        rewrite(&dir.join("a.txt"), "one\ntwo\n");
        rewrite(&dir.join(".env.local"), "KEY=new\n");
        rewrite(&dir.join(".ssh/config"), "Host b\n");
        let edits = compare(&dir, &before, &take(&dir)).unwrap();
        let diff = |path: &str| edits.iter().find(|e| e.path == path).unwrap().diff.clone();
        assert_eq!(diff("a.txt").unwrap().text, "@@ -1 +1,2 @@\n one\n+two\n");
        assert_eq!(diff(".env.local"), None);
        assert_eq!(diff(".ssh/config"), None);
        // The new text of a.txt is the "before" of its next change.
        let before = take(&dir);
        rewrite(&dir.join("a.txt"), "two\n");
        let edits = compare(&dir, &before, &take(&dir)).unwrap();
        assert_eq!(
            edits[0].diff.as_ref().unwrap().text,
            "@@ -1,2 +1 @@\n-one\n two\n"
        );
    }

    #[test]
    fn a_same_text_rewrite_is_no_change_and_a_long_diff_is_cut() {
        let dir = project("ledger-same");
        fs::write(dir.join("a"), "x\n").unwrap();
        seen::remember(&dir.join("a"), Stamp::of(&dir.join("a")).unwrap(), "x\n");
        let before = take(&dir);
        rewrite(&dir.join("a"), "x\n");
        assert_eq!(compare(&dir, &before, &take(&dir)).unwrap(), []);

        let long: String = (0..MAX_DIFF_LINES * 2).map(|n| format!("{n}\n")).collect();
        let cut = unified("", &long);
        assert!(cut.cut);
        assert!(cut.text.lines().count() <= MAX_DIFF_LINES + 1);
        assert!(cut.text.ends_with("more lines]\n"), "{}", cut.text);
    }
}
