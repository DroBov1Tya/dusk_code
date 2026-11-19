//! A change of a skill as lines for a client to draw.

use dusk_protocol::catalog::{DiffKind, DiffLine};
use similar::{ChangeTag, TextDiff};

use super::text::plain_diff_line;

/// Most lines a diff holds.
const MAX_LINES: usize = 400;
/// Most text bytes a diff holds.
const MAX_BYTES: usize = 64 * 1024;

/// The lines that turn `before` into `after`, each as plain text; `true` when it was
/// cut at the caps and more lines followed.
pub fn diff(before: &str, after: &str) -> (Vec<DiffLine>, bool) {
    let mut lines = Vec::new();
    let mut bytes = 0;
    for change in TextDiff::from_lines(before, after).iter_all_changes() {
        let text = plain_diff_line(change.value());
        if lines.len() == MAX_LINES || bytes + text.len() > MAX_BYTES {
            return (lines, true);
        }
        bytes += text.len();
        let kind = match change.tag() {
            ChangeTag::Equal => DiffKind::Same,
            ChangeTag::Insert => DiffKind::Added,
            ChangeTag::Delete => DiffKind::Removed,
        };
        lines.push(DiffLine { kind, text });
    }
    (lines, false)
}
