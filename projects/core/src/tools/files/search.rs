//! `search_files`: finds files by name and text in them, the way ripgrep does: ignored
//! and binary files are skipped. Each file passes the scope check, so what the file tools
//! may not touch is not found either.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    tools::{
        ToolScope,
        builtin::{parse_arguments, read_regular},
        tool::{Tool, ToolContext, ToolKind},
    },
};

const MAX_RESULTS: usize = 200;
const MAX_LINE_CHARS: usize = 300;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A search of a huge tree stops here and says so.
const TIME_LIMIT: Duration = Duration::from_secs(10);

pub struct SearchFiles;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    pattern: Option<String>,
    name: Option<String>,
    path: Option<String>,
}

impl Tool for SearchFiles {
    fn name(&self) -> &'static str {
        "search_files"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        format!(
            "Find text in files, or files by name; use this rather than grep or find in a \
             shell. `pattern` is a regular expression matched line by line, `(?i)` for any \
             case; `name` is a glob such as `*.rs`; give either or both. Files git ignores \
             and binary files are skipped. At most {MAX_RESULTS} results. Searches {} \
             unless `path` says where.",
            context.scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression." },
                "name": { "type": "string", "description": "Glob on file names." },
                "path": { "type": "string", "description": "File or folder to search." }
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope.clone();
        Box::pin(async move {
            let arguments: SearchArguments = parse_arguments(arguments)?;
            tokio::task::spawn_blocking(move || search(&arguments, &scope))
                .await
                .context("the search stopped")?
        })
    }
}

fn search(arguments: &SearchArguments, scope: &ToolScope) -> Result<String> {
    ensure!(
        arguments.pattern.is_some() || arguments.name.is_some(),
        "give a pattern, a name or both"
    );
    let pattern = arguments
        .pattern
        .as_deref()
        .map(Regex::new)
        .transpose()
        .context("invalid regular expression")?;
    let root = scope.resolve(arguments.path.as_deref().unwrap_or("."))?;
    let mut walk = WalkBuilder::new(&root);
    // Hidden files are searched, such as .github; git's own folder never is.
    walk.hidden(false).require_git(false);
    let mut overrides = OverrideBuilder::new(&root);
    overrides.add("!.git/").context("cannot skip .git")?;
    if let Some(name) = &arguments.name {
        overrides.add(name).context("invalid file name glob")?;
    }
    walk.overrides(overrides.build().context("invalid file name glob")?);

    let started = Instant::now();
    let mut found = Vec::new();
    let mut stopped = None;
    for entry in walk.build().flatten() {
        if started.elapsed() > TIME_LIMIT {
            stopped = Some("the search ran out of time");
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(path) = entry.path().to_str() else {
            continue;
        };
        if scope.resolve(path).is_err() {
            continue;
        }
        let shown = shown_path(entry.path(), scope.cwd());
        match &pattern {
            None => found.push(shown),
            Some(pattern) => matches(entry.path(), &shown, pattern, &mut found),
        }
        if found.len() >= MAX_RESULTS {
            found.truncate(MAX_RESULTS);
            stopped = Some("more results were left out; narrow the search");
            break;
        }
    }
    if found.is_empty() && stopped.is_none() {
        return Ok("nothing found".into());
    }
    let mut text = found.join("\n");
    if let Some(stopped) = stopped {
        text.push_str(&format!("\n[{stopped}]"));
    }
    Ok(text)
}

fn shown_path(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

/// A file too large, unreadable or not text has nothing to match.
fn matches(path: &Path, shown: &str, pattern: &Regex, found: &mut Vec<String>) {
    let Ok(bytes) = read_regular(path, MAX_FILE_BYTES) else {
        return;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return;
    };
    if text.contains('\0') {
        return;
    }
    for (index, line) in text.lines().enumerate() {
        if found.len() >= MAX_RESULTS {
            return;
        }
        if pattern.is_match(line) {
            let line: String = line.trim().chars().take(MAX_LINE_CHARS).collect();
            found.push(format!("{shown}:{}: {line}", index + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::testing::{temp_dir, tool_context};

    async fn run(scope: &ToolScope, arguments: Value) -> Result<String> {
        SearchFiles.call(arguments, tool_context(scope)).await
    }

    fn scope(name: &str) -> ToolScope {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(cwd.join("src")).unwrap();
        fs::create_dir_all(cwd.join(".git")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(cwd.join("src/a.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
        fs::write(cwd.join("notes.txt"), "alpha in text\n").unwrap();
        fs::write(cwd.join("data.bin"), b"alpha\0").unwrap();
        fs::write(cwd.join(".gitignore"), "skipped.txt\n").unwrap();
        fs::write(cwd.join("skipped.txt"), "alpha ignored\n").unwrap();
        fs::write(cwd.join(".git/config"), "alpha\n").unwrap();
        ToolScope::new(&cwd, &workspace).unwrap()
    }

    #[tokio::test]
    async fn finds_text_by_line_and_skips_ignored_and_binary_files() {
        let scope = scope("search_text");
        let found = run(&scope, json!({ "pattern": "alpha" })).await.unwrap();
        assert!(found.contains("src/a.rs:1: fn alpha() {}"), "{found}");
        assert!(found.contains("notes.txt:1:"), "{found}");
        for left_out in ["skipped.txt", "data.bin", ".git/"] {
            assert!(!found.contains(left_out), "{found}");
        }
    }

    #[tokio::test]
    async fn narrows_by_name_and_refuses_what_is_outside_the_scope() {
        let scope = scope("search_name");
        let found = run(&scope, json!({ "pattern": "fn", "name": "*.rs" }))
            .await
            .unwrap();
        assert_eq!(found.lines().count(), 2);
        assert_eq!(
            run(&scope, json!({ "name": "*.rs" })).await.unwrap(),
            "src/a.rs"
        );
        assert!(
            run(&scope, json!({ "name": "*", "path": "../.." }))
                .await
                .is_err()
        );
        assert!(run(&scope, json!({})).await.is_err());
        assert!(run(&scope, json!({ "pattern": "(" })).await.is_err());
    }

    #[tokio::test]
    async fn protected_files_are_not_found() {
        let scope = scope("search_protected");
        let secret = scope.cwd().join("src");
        let scope = scope.clone().protect(&secret);
        let found = run(&scope, json!({ "pattern": "alpha" })).await.unwrap();
        assert!(!found.contains("src/a.rs"), "{found}");
    }
}
