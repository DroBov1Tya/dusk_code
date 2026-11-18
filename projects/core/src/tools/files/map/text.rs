//! The answer as the model reads it: an indented tree whose deeper folders fold into
//! `name/ (N files)` until it fits the caps, or the paths a search found.

use std::{collections::BTreeMap, fmt::Write};

/// Most entries of one answer, one a line.
pub(super) const MAX_LINES: usize = 300;
/// Most bytes of one answer.
pub(super) const MAX_BYTES: usize = 12 * 1024;

/// The tree of `label`, the folder walked (`.` for the project); `paths` are relative
/// to it. Folders open level by level while the whole still fits; past that they fold.
pub(super) fn tree(label: &str, mut paths: Vec<String>) -> String {
    paths.sort_unstable();
    let root = Folder::of(&paths);
    let mut shown = Lines::default();
    for open in 0..=root.depth() {
        let mut lines = Lines::default();
        root.write(open, 1, &mut lines);
        // The first level is shown even past the caps, cut, so the model sees something.
        if lines.dropped > 0 && open > 0 {
            break;
        }
        shown = lines;
        if shown.dropped > 0 {
            break;
        }
    }
    let mut text = format!("{label}/ ({})\n{}", files(root.count), shown.text);
    if shown.dropped > 0 {
        let _ = writeln!(
            text,
            "[{} more entries; give `path` or `find`]",
            shown.dropped
        );
    }
    text
}

/// The paths a search found, relative to the project, sorted.
pub(super) fn found(mut paths: Vec<String>, find: &str) -> String {
    if paths.is_empty() {
        return format!("no path matches `{find}`");
    }
    paths.sort_unstable();
    let mut lines = Lines::default();
    for path in &paths {
        lines.push(0, path);
    }
    if lines.dropped > 0 {
        let _ = writeln!(
            lines.text,
            "[{} more; narrow `find` or give `path`]",
            lines.dropped
        );
    }
    lines.text
}

fn files(count: usize) -> String {
    match count {
        1 => "1 file".into(),
        count => format!("{count} files"),
    }
}

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    /// In the order of the sorted paths, so sorted.
    files: Vec<String>,
    /// Files under it at any depth.
    count: usize,
}

impl Folder {
    /// `paths` sorted, `/` between parts.
    fn of(paths: &[String]) -> Self {
        let mut root = Self::default();
        for path in paths {
            root.count += 1;
            let mut folder = &mut root;
            let mut parts = path.split('/').peekable();
            while let Some(part) = parts.next() {
                if parts.peek().is_none() {
                    folder.files.push(part.to_owned());
                } else {
                    folder = folder.folders.entry(part.to_owned()).or_default();
                    folder.count += 1;
                }
            }
        }
        root
    }

    /// Levels of folders beneath this one.
    fn depth(&self) -> usize {
        self.folders
            .values()
            .map(|folder| 1 + folder.depth())
            .max()
            .unwrap_or(0)
    }

    /// Folders first, then files. A folder at `level` is open while `level <= open`.
    fn write(&self, open: usize, level: usize, lines: &mut Lines) {
        for (name, folder) in &self.folders {
            if level <= open {
                lines.push(level, &format!("{name}/"));
                folder.write(open, level + 1, lines);
            } else {
                lines.push(level, &format!("{name}/ ({})", files(folder.count)));
            }
        }
        for name in &self.files {
            lines.push(level, name);
        }
    }
}

/// Text under the caps; what does not fit is only counted.
#[derive(Default)]
struct Lines {
    text: String,
    lines: usize,
    dropped: usize,
}

impl Lines {
    fn push(&mut self, indent: usize, entry: &str) {
        let width = indent * 2 + entry.len() + 1;
        if self.dropped > 0 || self.lines >= MAX_LINES || self.text.len() + width > MAX_BYTES {
            self.dropped += 1;
            return;
        }
        self.lines += 1;
        let _ = writeln!(self.text, "{:indent$}{entry}", "", indent = indent * 2);
    }
}
