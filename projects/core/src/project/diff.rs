//! A project's changes as a reader of a diff sees them: per file, with hunks and line
//! numbers. Tracked files come from `git diff` against the last commit; untracked files
//! are diffed against nothing, so they read as all added. Output is capped, so a huge
//! change cannot fill the client.

use std::path::Path;

use anyhow::{Result, ensure};
use dusk_protocol::project::{
    Change, ChangeKind, DiffKind, DiffLine, DiffStat, FileDiff, Hunk, Project,
};

use super::git::{changes, git, git_differs};

/// git's empty tree: the base in a repository with no commit yet.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const MAX_FILES: usize = 300;
const MAX_LINES: usize = 8000;
const MAX_LINE_CHARS: usize = 1000;
/// An untracked file larger than this is listed without its lines.
const MAX_UNTRACKED_BYTES: u64 = 256 * 1024;
const MAX_UNTRACKED_FILES: usize = 100;

/// The changes in the project, or in one file of it.
pub async fn diff(project: &Project, path: Option<&str>) -> Result<Vec<FileDiff>> {
    let dir = &project.dir;
    if let Some(path) = path {
        ensure!(
            !path.is_empty()
                && !path.starts_with('-')
                && Path::new(path).is_relative()
                && !path.split('/').any(|part| part == ".."),
            "not a path inside the project"
        );
    }
    let listed = changes(dir).await;
    let base = base(dir).await;
    let mut args = vec![
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-renames",
        "-U3",
        &base,
        "--",
    ];
    args.extend(path);
    let mut files = parse(&git(dir, &args).await?, &listed);
    let untracked = listed
        .iter()
        .filter(|change| change.kind == ChangeKind::Untracked)
        .filter(|change| path.is_none_or(|only| change.path == only))
        .take(MAX_UNTRACKED_FILES);
    for change in untracked {
        files.push(untracked_file(dir, &change.path).await);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.truncate(MAX_FILES);
    cap_lines(&mut files);
    Ok(files)
}

/// Lines added and removed in all changes, for a one-line summary.
pub async fn stat(dir: &Path, listed: &[Change]) -> DiffStat {
    let base = base(dir).await;
    let numstat = git(dir, &["diff", "--numstat", "--no-renames", &base, "--"])
        .await
        .unwrap_or_default();
    let mut stat = DiffStat::default();
    for line in numstat.lines() {
        let mut parts = line.split('\t');
        let (Some(added), Some(removed)) = (parts.next(), parts.next()) else {
            continue;
        };
        stat.files += 1;
        // A binary file shows "-": it adds no lines to the count.
        stat.added += added.parse::<u64>().unwrap_or(0);
        stat.removed += removed.parse::<u64>().unwrap_or(0);
    }
    for change in listed
        .iter()
        .filter(|change| change.kind == ChangeKind::Untracked)
        .take(MAX_UNTRACKED_FILES)
    {
        stat.files += 1;
        stat.added += count_lines(&dir.join(&change.path)).await;
    }
    stat
}

async fn base(dir: &Path) -> String {
    match git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).await {
        Ok(_) => "HEAD".to_owned(),
        Err(_) => EMPTY_TREE.to_owned(),
    }
}

async fn count_lines(path: &Path) -> u64 {
    let Ok(meta) = tokio::fs::symlink_metadata(path).await else {
        return 0;
    };
    if !meta.is_file() || meta.len() > MAX_UNTRACKED_BYTES {
        return 0;
    }
    match tokio::fs::read(path).await {
        Ok(bytes) if !bytes.contains(&0) => {
            let lines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
            lines + u64::from(bytes.last().is_some_and(|last| *last != b'\n'))
        }
        _ => 0,
    }
}

async fn untracked_file(dir: &Path, path: &str) -> FileDiff {
    let empty = |binary| FileDiff {
        path: path.to_owned(),
        kind: ChangeKind::Untracked,
        added: 0,
        removed: 0,
        binary,
        hunks: Vec::new(),
    };
    let full = dir.join(path);
    let Ok(meta) = tokio::fs::symlink_metadata(&full).await else {
        return empty(false);
    };
    if !meta.is_file() || meta.len() > MAX_UNTRACKED_BYTES {
        return empty(true);
    }
    let args = [
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-index",
        "--no-color",
        "--no-ext-diff",
        "-U3",
        "--",
        "/dev/null",
        path,
    ];
    match git_differs(dir, &args).await {
        Ok(text) => parse(&text, &[]).into_iter().next().map_or_else(
            || empty(false),
            |file| FileDiff {
                kind: ChangeKind::Untracked,
                path: path.to_owned(),
                ..file
            },
        ),
        Err(_) => empty(true),
    }
}

/// Keeps the first `MAX_LINES` lines in all; a file cut short keeps what it had.
fn cap_lines(files: &mut [FileDiff]) {
    let mut left = MAX_LINES;
    for file in files {
        for hunk in &mut file.hunks {
            hunk.lines.truncate(left);
            left -= hunk.lines.len();
        }
        file.hunks.retain(|hunk| !hunk.lines.is_empty());
    }
}

/// Reads git's unified diff. `listed` gives each file's kind; one it lacks is modified.
fn parse(text: &str, listed: &[Change]) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileDiff {
                path: header_path(rest),
                kind: ChangeKind::Modified,
                added: 0,
                removed: 0,
                binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(path) = line.strip_prefix("+++ b/") {
            file.path = path.to_owned();
        } else if line.starts_with("--- ") || line.starts_with("+++ ") {
            // `/dev/null` for a new or removed file: the path of the header stands.
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            file.binary = true;
        } else if line.starts_with("@@") {
            (old, new) = hunk_start(line);
            file.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
        } else if let Some(hunk) = file.hunks.last_mut() {
            let (kind, text) = match line.as_bytes().first() {
                Some(b'+') => (DiffKind::Added, &line[1..]),
                Some(b'-') => (DiffKind::Removed, &line[1..]),
                Some(b' ') => (DiffKind::Context, &line[1..]),
                // "\ No newline at end of file" and anything else git adds.
                _ => continue,
            };
            let text: String = text.chars().take(MAX_LINE_CHARS).collect();
            let entry = match kind {
                DiffKind::Added => {
                    file.added += 1;
                    new += 1;
                    DiffLine {
                        kind,
                        old: None,
                        new: Some(new - 1),
                        text,
                    }
                }
                DiffKind::Removed => {
                    file.removed += 1;
                    old += 1;
                    DiffLine {
                        kind,
                        old: Some(old - 1),
                        new: None,
                        text,
                    }
                }
                DiffKind::Context => {
                    old += 1;
                    new += 1;
                    DiffLine {
                        kind,
                        old: Some(old - 1),
                        new: Some(new - 1),
                        text,
                    }
                }
            };
            hunk.lines.push(entry);
        }
    }
    for file in &mut files {
        if let Some(change) = listed.iter().find(|change| change.path == file.path) {
            file.kind = change.kind;
        }
    }
    files
}

/// The path of `a/x b/x`, taken from the second half; a name with a space is cut at the
/// last ` b/`, which the `+++` line corrects whenever the file has text.
fn header_path(rest: &str) -> String {
    let tail = rest.rsplit_once(" b/").map_or(rest, |(_, path)| path);
    tail.trim_matches('"').to_owned()
}

/// The first old and new line numbers of `@@ -a,b +c,d @@`.
fn hunk_start(header: &str) -> (u32, u32) {
    let number = |sign: char| {
        header
            .split_whitespace()
            .find_map(|part| part.strip_prefix(sign))
            .and_then(|range| range.split(',').next())
            .and_then(|start| start.parse().ok())
            .unwrap_or(1)
    };
    (number('-'), number('+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@ fn main\n keep\n-old\n+new\n tail\ndiff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n";

    #[test]
    fn a_diff_reads_into_numbered_lines() {
        let files = parse(SAMPLE, &[]);
        assert_eq!(files.len(), 2);
        let file = &files[0];
        assert_eq!((file.added, file.removed), (1, 1));
        let lines = &file.hunks[0].lines;
        assert_eq!(
            (lines[1].kind, lines[1].old, lines[1].new),
            (DiffKind::Removed, Some(2), None)
        );
        assert_eq!(
            (lines[2].kind, lines[2].old, lines[2].new),
            (DiffKind::Added, None, Some(2))
        );
        assert_eq!(lines[3].new, Some(3));
        assert!(files[1].binary && files[1].hunks.is_empty());
    }

    #[test]
    fn a_path_that_leaves_the_project_is_refused() {
        let project = Project {
            id: "p".into(),
            name: "p".into(),
            description: String::new(),
            dir: std::env::temp_dir(),
            created_at: 0,
        };
        let run = |path: &'static str| {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(diff(&project, Some(path)))
        };
        for bad in ["../x", "/etc/passwd", "-p", "a/../../b", ""] {
            assert!(run(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_repository_gives_modified_and_untracked_files() {
        let dir = crate::testing::temp_dir("diff-repo");
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&dir)
                .output();
            status.map(|out| out.status.success()).unwrap_or(false)
        };
        if !run(&["init", "-q"]) {
            return;
        }
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        assert!(run(&["add", "a.txt"]) && run(&["commit", "-qm", "first"]));
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\nfour\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x\ny\n").unwrap();
        let project = Project {
            id: "p".into(),
            name: "p".into(),
            description: String::new(),
            dir: dir.clone(),
            created_at: 0,
        };
        let files = diff(&project, None).await.unwrap();
        let by = |path: &str| files.iter().find(|file| file.path == path).unwrap();
        assert_eq!((by("a.txt").added, by("a.txt").removed), (2, 1));
        assert_eq!(by("a.txt").kind, ChangeKind::Modified);
        assert_eq!(
            (by("new.txt").added, by("new.txt").kind),
            (2, ChangeKind::Untracked)
        );
        let listed = changes(&dir).await;
        let stat = stat(&dir, &listed).await;
        assert_eq!((stat.files, stat.added, stat.removed), (2, 4, 1));
    }
}
