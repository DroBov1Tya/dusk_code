//! `search_files`: finds files by name and text in them, the way ripgrep does: ignored
//! and binary files are skipped. Each file passes the scope check, so what the file tools
//! may not touch is not found either. The walk follows no link, so a file's path needs no
//! resolving before the check.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::init::files::read_regular;
use crate::{
    BoxFuture,
    tools::{
        Place, ToolScope, schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

const MAX_RESULTS: usize = 200;
const MAX_LINE_CHARS: usize = 300;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A search of a huge tree stops here and says so.
const TIME_LIMIT: Duration = Duration::from_secs(10);

pub struct SearchFiles;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    /// Regular expression.
    pattern: Option<String>,
    /// Glob on file names.
    name: Option<String>,
    /// File or folder to search.
    path: Option<String>,
}

impl Tool for SearchFiles {
    fn name(&self) -> &'static str {
        "search_files"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, context: &ToolContext) -> String {
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
        schema::of::<SearchArguments>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        let scope = context.scope.clone();
        Box::pin(async move {
            let arguments: SearchArguments = parse_arguments(arguments)?;
            let root = context
                .resolve(arguments.path.as_deref().unwrap_or("."), false)
                .await?;
            tokio::task::spawn_blocking(move || search(&arguments, &root, &scope))
                .await
                .context("the search stopped")?
        })
    }
}

/// `root` is resolved and, when outside the folders, approved; what is under it may be
/// searched, the config folder never.
fn search(arguments: &SearchArguments, root: &Path, scope: &ToolScope) -> Result<String> {
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
    let mut walk = WalkBuilder::new(root);
    // Hidden files are searched, such as .github; git's own folder never is. Links are
    // not followed, so a walked path under the resolved root is resolved too.
    walk.hidden(false).require_git(false).follow_links(false);
    let mut overrides = OverrideBuilder::new(root);
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
        // A link is not a file here, so nothing found leads elsewhere.
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if entry.path().to_str().is_none() {
            continue;
        }
        let open = match scope.place(entry.path()) {
            Ok(Place::Inside) => true,
            Ok(Place::Outside) => entry.path().starts_with(root),
            Err(_) => false,
        };
        if !open {
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

    /// The check reads walked paths as they are, so a link must never lead it past the
    /// protected folder or out of the root.
    #[cfg(unix)]
    #[tokio::test]
    async fn links_to_protected_or_outside_files_are_not_followed() {
        let scope = scope("search_links");
        let root = scope.cwd().parent().unwrap().to_owned();
        let secret = root.join("config");
        fs::create_dir_all(&secret).unwrap();
        fs::write(secret.join("keys.yml"), "alpha key\n").unwrap();
        fs::write(root.join("outside.txt"), "alpha outside\n").unwrap();
        let scope = scope.clone().protect(&secret);
        let cwd = scope.cwd();
        std::os::unix::fs::symlink(secret.join("keys.yml"), cwd.join("keys.yml")).unwrap();
        std::os::unix::fs::symlink(&secret, cwd.join("config")).unwrap();
        std::os::unix::fs::symlink(root.join("outside.txt"), cwd.join("out.txt")).unwrap();
        let found = run(&scope, json!({ "pattern": "alpha" })).await.unwrap();
        for left_out in ["key", "outside"] {
            assert!(!found.contains(left_out), "{found}");
        }
        assert!(found.contains("src/a.rs:1:"), "{found}");
        let named = run(&scope, json!({ "name": "*.yml" })).await.unwrap();
        assert_eq!(named, "nothing found");
    }

    /// A protected folder inside the searched one is skipped though no path is resolved.
    #[tokio::test]
    async fn a_protected_folder_inside_the_root_is_skipped_but_a_shared_one_inside_it_is_not() {
        let scope = scope("search_protected_inside");
        let config = scope.cwd().join("config");
        fs::create_dir_all(config.join("skills")).unwrap();
        fs::write(config.join("keys.yml"), "alpha key\n").unwrap();
        fs::write(config.join("skills/SKILL.md"), "alpha skill\n").unwrap();
        let scope = scope.clone().protect(&config).share(&config.join("skills"));
        let found = run(&scope, json!({ "pattern": "alpha" })).await.unwrap();
        assert!(!found.contains("keys.yml"), "{found}");
        assert!(found.contains("config/skills/SKILL.md:1:"), "{found}");
    }
}
