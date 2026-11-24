//! The map as the model reads it: an indented tree whose deeper folders fold into
//! `name/ (N files, M lines)` until it fits the caps, or the paths a search found. A
//! file shows its lines and the start of its summary when the graph has them.

use std::{collections::BTreeMap, fmt::Write};

use crate::state::graph::MapEntry;

/// Most entries of one answer, one a line.
pub(super) const MAX_LINES: usize = 300;
/// Most bytes of one answer.
pub(super) const MAX_BYTES: usize = 12 * 1024;
/// Characters of a summary the map shows; `about` shows it whole.
const MAP_SUMMARY_CHARS: usize = 90;

/// A file of the map, its path relative to the project.
pub(super) struct Entry {
    pub path: String,
    /// `None` when the folder has no graph: the map is by names alone.
    pub lines: Option<u64>,
    pub summary: String,
    pub stale: bool,
}

impl Entry {
    pub fn named(path: String) -> Self {
        Self {
            path,
            lines: None,
            summary: String::new(),
            stale: false,
        }
    }

    /// The name, lines and the start of the summary, as one line of the map.
    fn line(&self, name: &str) -> String {
        let mut line = name.to_owned();
        if let Some(lines) = self.lines {
            let _ = write!(line, " {lines}");
        }
        if !self.summary.is_empty() {
            let _ = write!(line, ": {}", clipped(&self.summary, MAP_SUMMARY_CHARS));
            if self.stale {
                line.push_str(" (stale)");
            }
        }
        line
    }
}

impl From<MapEntry> for Entry {
    fn from(entry: MapEntry) -> Self {
        Self {
            path: entry.path,
            lines: Some(entry.lines),
            summary: entry.summary,
            stale: entry.stale,
        }
    }
}

/// The tree of `folder` (the project for `None`); `entries` are under it. Folders open
/// level by level while the whole still fits; past that they fold.
pub(super) fn tree(folder: Option<&str>, mut entries: Vec<Entry>) -> String {
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let prefix = folder.map(|folder| format!("{folder}/"));
    let root = Folder::of(&entries, prefix.as_deref());
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
    let label = folder.unwrap_or(".");
    let mut text = format!("{label}/ ({})\n{}", root.totals(), shown.text);
    if shown.dropped > 0 {
        let _ = writeln!(
            text,
            "[{} more entries; give `path` or `find`]",
            shown.dropped
        );
    }
    text
}

/// The files a search found, relative to the project, sorted.
pub(super) fn found(mut entries: Vec<Entry>, find: &str) -> String {
    if entries.is_empty() {
        return format!("no path matches `{find}`");
    }
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let mut lines = Lines::default();
    for entry in &entries {
        lines.push(0, &entry.line(&entry.path));
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

/// At most `cap` characters of `text`, `…` at the cut.
pub(super) fn clipped(text: &str, cap: usize) -> String {
    match text.char_indices().nth(cap) {
        None => text.to_owned(),
        Some((at, _)) => format!("{}…", text[..at].trim_end()),
    }
}

#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<&'a str, Folder<'a>>,
    /// In the order of the sorted paths, so sorted.
    files: Vec<(&'a str, &'a Entry)>,
    /// Files under it at any depth.
    count: usize,
    /// Their lines, when the graph counted them.
    lines: Option<u64>,
}

impl<'a> Folder<'a> {
    /// `entries` sorted; `prefix` is cut from each path.
    fn of(entries: &'a [Entry], prefix: Option<&str>) -> Self {
        let mut root = Self::default();
        for entry in entries {
            let path = prefix
                .and_then(|prefix| entry.path.strip_prefix(prefix))
                .unwrap_or(&entry.path);
            root.add(entry);
            let mut folder = &mut root;
            let mut parts = path.split('/').peekable();
            while let Some(part) = parts.next() {
                if parts.peek().is_none() {
                    folder.files.push((part, entry));
                } else {
                    folder = folder.folders.entry(part).or_default();
                    folder.add(entry);
                }
            }
        }
        root
    }

    fn add(&mut self, entry: &Entry) {
        self.count += 1;
        if let Some(lines) = entry.lines {
            self.lines = Some(self.lines.unwrap_or(0).saturating_add(lines));
        }
    }

    fn totals(&self) -> String {
        let files = match self.count {
            1 => "1 file".into(),
            count => format!("{count} files"),
        };
        match self.lines {
            Some(lines) => format!("{files}, {lines} lines"),
            None => files,
        }
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
                match folder.lines {
                    Some(count) => lines.push(level, &format!("{name}/ {count}")),
                    None => lines.push(level, &format!("{name}/")),
                }
                folder.write(open, level + 1, lines);
            } else {
                lines.push(level, &format!("{name}/ ({})", folder.totals()));
            }
        }
        for (name, entry) in &self.files {
            lines.push(level, &entry.line(name));
        }
    }
}

/// Text under the caps; what does not fit is only counted.
#[derive(Default)]
pub(super) struct Lines {
    pub text: String,
    lines: usize,
    pub dropped: usize,
}

impl Lines {
    pub fn push(&mut self, indent: usize, entry: &str) {
        let width = indent * 2 + entry.len() + 1;
        if self.dropped > 0 || self.lines >= MAX_LINES || self.text.len() + width > MAX_BYTES {
            self.dropped += 1;
            return;
        }
        self.lines += 1;
        let _ = writeln!(self.text, "{:indent$}{entry}", "", indent = indent * 2);
    }
}

impl Lines {
    /// The text, with a line for what did not fit.
    pub fn finish(mut self) -> String {
        if self.dropped > 0 {
            let _ = writeln!(self.text, "[{} more lines; ask about less]", self.dropped);
        }
        self.text
    }
}
