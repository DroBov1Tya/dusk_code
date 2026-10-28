//! Plain substring search over the files under a directory.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    builtin::{parse_arguments, printable, read_regular},
    scope::ToolScope,
    tool::{Tool, ToolFuture},
};

const SKIPPED_DIRS: [&str; 3] = [".git", "target", "node_modules"];
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_MATCHES: usize = 200;
const MAX_LINE_CHARS: usize = 300;
const MAX_FILES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DURATION: Duration = Duration::from_secs(10);

pub(super) struct Search;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    pattern: String,
    #[serde(default = "current_dir")]
    path: String,
}

impl Tool for Search {
    fn name(&self) -> &'static str {
        "search"
    }

    fn description(&self, scope: &ToolScope) -> String {
        format!(
            "Find lines containing a plain substring (not a regex) in text files under a \
             directory or in one file. Skips .git, target and node_modules. Prints \
             path:line: text. Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Exact substring to find." },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search. Defaults to the working directory."
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, scope: &'a ToolScope) -> ToolFuture<'a> {
        Box::pin(async move {
            let arguments: SearchArguments = parse_arguments(arguments)?;
            ensure!(!arguments.pattern.is_empty(), "pattern is empty");
            let root = scope.resolve(&arguments.path)?;
            let cwd = scope.cwd().to_owned();
            let pattern = arguments.pattern;
            // A cancelled turn drops this future; the guard then stops the blocking walk.
            let stop = StopOnDrop(Arc::new(AtomicBool::new(false)));
            let stopped = Arc::clone(&stop.0);
            tokio::task::spawn_blocking(move || search(&root, &cwd, &pattern, &stopped))
                .await
                .context("search failed")?
        })
    }
}

fn current_dir() -> String {
    ".".into()
}

struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Iterative and without following symlinks, so a link loop or a link out of the scope
/// is never walked. Bounded in files, bytes and time, so a huge tree cannot stall the turn.
fn search(root: &Path, cwd: &Path, pattern: &str, stopped: &AtomicBool) -> Result<String> {
    fs::symlink_metadata(root).context("cannot read search path")?;
    let deadline = Instant::now() + MAX_DURATION;
    let (mut files, mut bytes) = (0, 0);
    let mut output = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        if stopped.load(Ordering::Relaxed) {
            return Ok("search cancelled".into());
        }
        if files >= MAX_FILES || bytes >= MAX_TOTAL_BYTES || Instant::now() >= deadline {
            output.push("[search stopped at its size or time limit; narrow the path]".into());
            break;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            pending.extend(children(&path));
        } else if metadata.is_file() && metadata.len() <= MAX_FILE_BYTES {
            files += 1;
            bytes += metadata.len();
            if !search_file(&path, cwd, pattern, &mut output) {
                output.push("[more matches omitted]".into());
                break;
            }
        }
    }
    if output.is_empty() {
        return Ok("no matches".into());
    }
    Ok(output.join("\n"))
}

/// Sorted in reverse so popping from the stack visits them in order.
fn children(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| !SKIPPED_DIRS.iter().any(|name| entry.file_name() == *name))
        .map(|entry| entry.path())
        .collect();
    paths.sort_by(|a, b| b.cmp(a));
    paths
}

/// Returns false once the match limit is reached.
fn search_file(path: &Path, cwd: &Path, pattern: &str, output: &mut Vec<String>) -> bool {
    let Ok(bytes) = read_regular(path, MAX_FILE_BYTES) else {
        return true;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return true;
    };
    let shown = printable(&path.strip_prefix(cwd).unwrap_or(path).to_string_lossy());
    for (index, line) in text.lines().enumerate() {
        if !line.contains(pattern) {
            continue;
        }
        if output.len() == MAX_MATCHES {
            return false;
        }
        output.push(format!(
            "{shown}:{}: {}",
            index + 1,
            printable(trimmed(line))
        ));
    }
    true
}

fn trimmed(line: &str) -> &str {
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((end, _)) => &line[..end],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    fn scope(name: &str) -> ToolScope {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        ToolScope::new(&cwd, &workspace).unwrap()
    }

    #[tokio::test]
    async fn finds_matches_with_line_numbers() {
        let scope = scope("search_matches");
        fs::create_dir_all(scope.cwd().join("src")).unwrap();
        fs::write(
            scope.cwd().join("src/a.rs"),
            "fn main() {}\nlet needle = 1;\n",
        )
        .unwrap();
        fs::write(scope.cwd().join("b.txt"), "needle\nhay\n").unwrap();
        let output = Search
            .call(json!({ "pattern": "needle" }), &scope)
            .await
            .unwrap();
        assert_eq!(output, "b.txt:1: needle\nsrc/a.rs:2: let needle = 1;");
    }

    #[tokio::test]
    async fn skips_git_and_binary_files() {
        let scope = scope("search_skips");
        fs::create_dir_all(scope.cwd().join(".git")).unwrap();
        fs::write(scope.cwd().join(".git/config"), "needle").unwrap();
        fs::write(scope.cwd().join("bin"), [b'n', 0xff, b'e']).unwrap();
        let output = Search
            .call(json!({ "pattern": "needle", "path": "." }), &scope)
            .await
            .unwrap();
        assert_eq!(output, "no matches");
    }

    #[tokio::test]
    async fn caps_matches_and_line_length() {
        let scope = scope("search_caps");
        let long = "x".repeat(400);
        let text = format!("{long}\n").repeat(MAX_MATCHES + 1);
        fs::write(scope.cwd().join("a.txt"), text).unwrap();
        let output = Search
            .call(json!({ "pattern": "x" }), &scope)
            .await
            .unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), MAX_MATCHES + 1);
        assert_eq!(lines[MAX_MATCHES], "[more matches omitted]");
        assert_eq!(lines[0].len(), "a.txt:1: ".len() + MAX_LINE_CHARS);
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_scope() {
        let scope = scope("search_outside");
        let arguments = json!({ "pattern": "x", "path": "/etc" });
        assert!(Search.call(arguments, &scope).await.is_err());
    }
}
