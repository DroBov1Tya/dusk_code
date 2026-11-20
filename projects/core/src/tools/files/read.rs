//! `read_file`: a UTF-8 text file, a window of lines at a time, so one read cannot
//! flood the context.

use anyhow::{Result, bail, ensure};
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    project::ledger::{self, Stamp},
    tools::{
        registry::MAX_RESULT_BYTES,
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind, printable},
    },
};

use super::read_text;

/// Lines one read returns when the model does not ask for fewer.
const MAX_LINES: usize = 2000;
/// Bytes one read returns at most; the window ends at the last whole line before it.
const MAX_BYTES: usize = 50 * 1024;

/// Files one call may read together.
const MIN_PATHS: usize = 2;
const MAX_PATHS: usize = 8;
/// What a section of the many-files form takes besides the file's text: its note for a
/// cut, the line that ends it.
const SECTION_NOTE: usize = 160;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    /// The file.
    path: Option<String>,
    /// 2 to 8 files, each read from its start.
    paths: Option<Vec<String>>,
    /// The first line to read, counting from 1.
    offset: Option<usize>,
    /// How many lines to read.
    limit: Option<usize>,
}

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, context: &ToolContext) -> String {
        let scope = &*context.scope;
        format!(
            "Read a UTF-8 text file. Returns up to {MAX_LINES} lines or 50 KiB from `offset` \
             (1-based, default 1); a note at the end says where to read on. For a large \
             file, find the place with search_files first and read only that part. Read a \
             file before you change it; do not read one again that you just wrote. \
             Relative paths resolve against {}.{}",
            scope.cwd().display(),
            if reads_many(context) {
                " Several files at once: `paths` (2-8) instead of `path`, each from its start."
            } else {
                ""
            }
        )
    }

    /// The schema a call is checked against: it accepts `paths`, and `call` refuses it
    /// while the switch is off.
    fn parameters(&self) -> Value {
        schema::of::<ReadArguments>()
    }

    /// With the switch off the model sees the tool as it was before `paths`.
    fn offered_parameters(&self, context: &ToolContext) -> Value {
        let mut schema = self.parameters();
        if !reads_many(context) {
            schema["properties"]
                .as_object_mut()
                .map(|properties| properties.remove("paths"));
            schema["required"] = json!(["path"]);
        }
        schema
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: ReadArguments = parse_arguments(arguments)?;
            match (&arguments.path, &arguments.paths) {
                (Some(path), None) => {
                    let path = context.resolve(path, false).await?;
                    let text = read_and_keep(&context, &path).await?;
                    Ok(window(&text, arguments.offset, arguments.limit))
                }
                (None, Some(paths)) => {
                    ensure!(
                        reads_many(&context),
                        "`paths` is not available: give `path`"
                    );
                    ensure!(
                        arguments.offset.is_none() && arguments.limit.is_none(),
                        "`offset` and `limit` go with `path`; read a file that needs a window alone"
                    );
                    read_many(paths, &context).await
                }
                (Some(_), Some(_)) => bail!("give either `path` or `paths`, not both"),
                (None, None) => bail!("give `path`, or `paths` for several files"),
            }
        })
    }
}

fn reads_many(context: &ToolContext) -> bool {
    context.config.limits().read_many_files
}

/// Each file in a section headed by its path, in the order asked. A file that cannot be
/// read says so in its section and does not fail the others.
async fn read_many(paths: &[String], context: &ToolContext) -> Result<String> {
    ensure!(
        (MIN_PATHS..=MAX_PATHS).contains(&paths.len()),
        "`paths` takes {MIN_PATHS} to {MAX_PATHS} files, got {}; use `path` for one",
        paths.len()
    );
    let mut texts = Vec::new();
    for path in paths {
        // The same check as a single `path`, approval of a place outside included.
        texts.push(match context.resolve(path, false).await {
            Ok(resolved) => read_and_keep(context, &resolved).await,
            Err(error) => Err(error),
        });
    }
    let headers: Vec<String> = paths
        .iter()
        .map(|path| format!("==> {} <==\n", printable(path)))
        .collect();
    let overhead: usize = headers
        .iter()
        .map(|header| header.len() + SECTION_NOTE)
        .sum();
    // What each file would take alone, so a small one gives its unused share away.
    let needs: Vec<usize> = texts
        .iter()
        .map(|text| {
            text.as_ref()
                .map_or(0, |text| window(text, None, None).len())
        })
        .collect();
    let allowed = shares(&needs, MAX_RESULT_BYTES.saturating_sub(overhead));
    let mut out = String::new();
    for (index, header) in headers.iter().enumerate() {
        out.push_str(header);
        match &texts[index] {
            Ok(text) if text.is_empty() => out.push_str("(empty)"),
            Ok(text) => out.push_str(&section(text, allowed[index])),
            Err(error) => {
                let message = format!("{error:#}");
                let line = message.lines().next().unwrap_or_default();
                out.push_str(&format!("error: {line}"));
            }
        }
        out.push('\n');
    }
    Ok(out)
}

/// Max-min fair split of `budget`: each file gets what it needs, or an equal share of
/// what the smaller ones left.
fn shares(needs: &[usize], budget: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..needs.len()).collect();
    order.sort_by_key(|&index| needs[index]);
    let mut shares = vec![0; needs.len()];
    let mut left = budget;
    for (done, &index) in order.iter().enumerate() {
        let give = needs[index].min(left / (needs.len() - done));
        shares[index] = give;
        left -= give;
    }
    shares
}

/// The file's default window, cut to `allowance` bytes when it is larger; the note at
/// the end says where to read on with `path` and `offset`.
fn section(text: &str, allowance: usize) -> String {
    let whole = window(text, None, None);
    if whole.len() <= allowance {
        return whole;
    }
    let mut out = window_within(text, None, None, allowance);
    // One line longer than the share has no whole line to end at. The note of a cut at a
    // line end is short, so only more than that is a line to cut.
    if out.len() > allowance + SECTION_NOTE / 2 {
        out.truncate(out.floor_char_boundary(allowance));
        out.push_str("\n[cut; read this file alone with path]");
    }
    out
}

/// Reads a file and keeps its text for the ledger. The stamp is taken before the read: a
/// write in between leaves a stamp the text does not match, so the text is never used as
/// a "before" it was not.
async fn read_and_keep(context: &ToolContext, path: &Path) -> Result<String> {
    let stamp = tokio::fs::symlink_metadata(path)
        .await
        .ok()
        .and_then(|meta| Stamp::from_metadata(&meta));
    let text = read_text(path).await?;
    if let Some(stamp) = stamp {
        keep_seen(context, path, stamp, &text);
    }
    Ok(text)
}

/// Keeps the text of a project file as the "before" of its next change, for the ledger.
fn keep_seen(context: &ToolContext, path: &Path, stamp: Stamp, text: &str) {
    if !context.config.limits().record_changes {
        return;
    }
    let Some(relative) = path
        .strip_prefix(context.scope.cwd())
        .ok()
        .and_then(Path::to_str)
    else {
        return;
    };
    if ledger::may_keep_text(relative) {
        ledger::seen::remember(path, stamp, text);
    }
}

/// The lines from `offset` (1-based), at most `limit` of them and `MAX_BYTES` in all,
/// with a note when the file goes on. A whole small file comes back unchanged.
fn window(text: &str, offset: Option<usize>, limit: Option<usize>) -> String {
    window_within(text, offset, limit, MAX_BYTES)
}

/// `window` with its own byte cap.
fn window_within(
    text: &str,
    offset: Option<usize>,
    limit: Option<usize>,
    max_bytes: usize,
) -> String {
    let total = text.lines().count();
    let first = offset.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(MAX_LINES).clamp(1, MAX_LINES);
    if first == 1 && total <= limit && text.len() <= max_bytes {
        return text.to_owned();
    }
    if first > total {
        return format!("[the file has {total} lines; offset {first} is past its end]");
    }
    let mut out = String::new();
    let mut last = first - 1;
    for line in text.lines().skip(first - 1).take(limit) {
        if out.len() + line.len() + 1 > max_bytes && last >= first {
            break;
        }
        out.push_str(line);
        out.push('\n');
        last += 1;
    }
    if last < total {
        out.push_str(&format!(
            "[lines {first}-{last} of {total}; read on with offset {}]",
            last + 1
        ));
    } else if first > 1 {
        out.push_str(&format!("[lines {first}-{last} of {total}; end of file]"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(lines: usize) -> String {
        (1..=lines).map(|n| format!("line {n}\n")).collect()
    }

    #[test]
    fn a_small_file_comes_back_whole() {
        let text = numbered(3);
        assert_eq!(window(&text, None, None), text);
    }

    #[test]
    fn a_window_names_where_to_read_on() {
        let out = window(&numbered(10), Some(3), Some(2));
        assert_eq!(
            out,
            "line 3\nline 4\n[lines 3-4 of 10; read on with offset 5]"
        );
    }

    #[test]
    fn a_long_file_is_cut_at_the_line_limit() {
        let out = window(&numbered(MAX_LINES + 5), None, None);
        assert!(out.ends_with(&format!("read on with offset {}]", MAX_LINES + 1)));
    }

    #[test]
    fn the_byte_cap_ends_at_a_whole_line() {
        let line = "x".repeat(1000);
        let text = format!("{line}\n").repeat(100);
        let out = window(&text, None, None);
        assert!(out.len() < MAX_BYTES + 100);
        assert!(out.contains("read on with offset 52]"));
    }

    #[test]
    fn an_offset_past_the_end_says_so() {
        assert!(window(&numbered(2), Some(9), None).contains("past its end"));
    }

    #[test]
    fn the_last_window_marks_the_end() {
        let out = window(&numbered(5), Some(4), None);
        assert_eq!(out, "line 4\nline 5\n[lines 4-5 of 5; end of file]");
    }

    mod many {
        use std::fs;

        use super::super::*;
        use crate::testing::{temp_dir, tool_context_with};
        use crate::tools::ToolScope;

        const ON: &str = "limits:\n  read_many_files: true\n";

        /// A project folder with the files `(name, text)`.
        fn project(name: &str, files: &[(&str, &[u8])]) -> ToolScope {
            let root = temp_dir(name);
            let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
            fs::create_dir_all(&cwd).unwrap();
            fs::create_dir_all(&workspace).unwrap();
            for (file, bytes) in files {
                fs::write(cwd.join(file), bytes).unwrap();
            }
            fs::write(root.join("secret"), "outside").unwrap();
            ToolScope::new(&cwd, &workspace).unwrap()
        }

        async fn read(scope: &ToolScope, arguments: Value) -> Result<String> {
            ReadFile.call(arguments, tool_context_with(scope, ON)).await
        }

        #[tokio::test]
        async fn two_files_come_back_in_the_order_asked() {
            let scope = project("many_order", &[("a.txt", b"alpha\n"), ("b.txt", b"beta\n")]);
            let out = read(&scope, json!({ "paths": ["b.txt", "a.txt"] }))
                .await
                .unwrap();
            assert_eq!(out, "==> b.txt <==\nbeta\n\n==> a.txt <==\nalpha\n\n");
        }

        #[tokio::test]
        async fn a_missing_file_fails_only_its_own_section() {
            let scope = project("many_missing", &[("a.txt", b"alpha"), ("c.txt", b"gamma")]);
            let out = read(&scope, json!({ "paths": ["a.txt", "b.txt", "c.txt"] }))
                .await
                .unwrap();
            assert!(out.contains("==> a.txt <==\nalpha\n"), "{out}");
            assert!(out.contains("==> c.txt <==\ngamma\n"), "{out}");
            let section = out.split("==> b.txt <==\n").nth(1).unwrap();
            assert!(section.starts_with("error: "), "{out}");
        }

        #[tokio::test]
        async fn a_path_outside_the_scope_is_refused_as_alone() {
            let scope = project("many_outside", &[("a.txt", b"alpha")]);
            let paths = json!({ "paths": ["a.txt", "../secret"] });
            let out = read(&scope, paths).await.unwrap();
            assert!(out.contains("==> a.txt <==\nalpha\n"), "{out}");
            assert!(
                !out.contains("outside\n"),
                "the file outside was read: {out}"
            );
            let alone = read(&scope, json!({ "path": "../secret" })).await;
            let alone = format!("{:#}", alone.unwrap_err());
            assert!(out.contains(&format!("error: {alone}")), "{out}");
        }

        #[tokio::test]
        async fn path_and_paths_together_or_neither_is_an_error() {
            let scope = project("many_both", &[("a.txt", b"a"), ("b.txt", b"b")]);
            let both = json!({ "path": "a.txt", "paths": ["a.txt", "b.txt"] });
            let error = read(&scope, both).await.unwrap_err();
            assert!(format!("{error:#}").contains("not both"));
            let error = read(&scope, json!({})).await.unwrap_err();
            assert!(format!("{error:#}").contains("`path`"));
            let windowed = json!({ "paths": ["a.txt", "b.txt"], "offset": 2 });
            assert!(read(&scope, windowed).await.is_err());
        }

        #[tokio::test]
        async fn one_path_and_nine_are_refused_two_to_eight_are_read() {
            let files: Vec<String> = (0..9).map(|n| format!("f{n}.txt")).collect();
            let named: Vec<(&str, &[u8])> = files.iter().map(|f| (f.as_str(), &b"x"[..])).collect();
            let scope = project("many_count", &named);
            for count in [1, 9] {
                let error = read(&scope, json!({ "paths": &files[..count] })).await;
                let message = format!("{:#}", error.unwrap_err());
                assert!(message.contains("2 to 8"), "{message}");
            }
            let out = read(&scope, json!({ "paths": &files[..8] })).await.unwrap();
            assert_eq!(out.matches("==> ").count(), 8);
        }

        #[tokio::test]
        async fn a_big_file_does_not_starve_a_small_one_and_shares_are_equal() {
            let line = format!("{}\n", "x".repeat(99));
            let big = line.repeat(1000);
            let scope = project(
                "many_budget",
                &[
                    ("big1.txt", big.as_bytes()),
                    ("small.txt", b"tiny\n"),
                    ("big2.txt", big.as_bytes()),
                ],
            );
            let paths = json!({ "paths": ["big1.txt", "small.txt", "big2.txt"] });
            let out = read(&scope, paths).await.unwrap();
            assert!(out.len() <= MAX_RESULT_BYTES, "{}", out.len());
            assert!(out.contains("==> small.txt <==\ntiny\n"), "small file cut");
            let sections: Vec<&str> = out.split("==> ").skip(1).collect();
            let (first, last) = (sections[0].len(), sections[2].len());
            assert!(first.abs_diff(last) < 200, "{first} vs {last}");
            assert!(first > 20 * 1024, "{first}");
            for section in [sections[0], sections[2]] {
                assert!(section.contains("read on with offset"), "no note: cut");
            }
        }

        #[tokio::test]
        async fn one_huge_line_stays_inside_its_share() {
            let long = "y".repeat(500 * 1024);
            let scope = project("many_line", &[("l.txt", long.as_bytes()), ("s.txt", b"s")]);
            let out = read(&scope, json!({ "paths": ["l.txt", "s.txt"] }))
                .await
                .unwrap();
            assert!(out.len() <= MAX_RESULT_BYTES, "{}", out.len());
            assert!(out.contains("[cut; read this file alone with path]"));
            assert!(out.contains("==> s.txt <==\ns\n"));
        }

        #[tokio::test]
        async fn a_binary_file_gets_its_own_error() {
            let scope = project(
                "many_binary",
                &[("a.txt", b"alpha"), ("b.bin", &[0xff, 0xfe, 0x00, 0x80])],
            );
            let out = read(&scope, json!({ "paths": ["b.bin", "a.txt"] }))
                .await
                .unwrap();
            assert!(out.contains("==> b.bin <==\nerror: file is not valid UTF-8 text\n"));
            assert!(out.contains("==> a.txt <==\nalpha\n"));
        }

        #[tokio::test]
        async fn paths_need_the_switch() {
            let scope = project("many_off", &[("a.txt", b"a"), ("b.txt", b"b")]);
            let paths = json!({ "paths": ["a.txt", "b.txt"] });
            let error = ReadFile
                .call(
                    paths,
                    tool_context_with(&scope, "limits:\n  read_many_files: false\n"),
                )
                .await
                .unwrap_err();
            assert!(format!("{error:#}").contains("not available"));
        }

        /// What the model was shown before `paths` existed.
        #[derive(Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Before {
            /// The file.
            path: String,
            /// The first line to read, counting from 1.
            offset: Option<usize>,
            /// How many lines to read.
            limit: Option<usize>,
        }

        #[test]
        fn the_spec_is_unchanged_while_the_switch_is_off() {
            let scope = project("many_spec", &[]);
            let off = tool_context_with(&scope, "limits:\n  read_many_files: false\n");
            assert_eq!(ReadFile.offered_parameters(&off), schema::of::<Before>());
            let before = format!(
                "Read a UTF-8 text file. Returns up to {MAX_LINES} lines or 50 KiB from `offset` \
                 (1-based, default 1); a note at the end says where to read on. For a large \
                 file, find the place with search_files first and read only that part. Read a \
                 file before you change it; do not read one again that you just wrote. \
                 Relative paths resolve against {}.",
                scope.cwd().display()
            );
            assert_eq!(ReadFile.description(&off), before);
            let on = tool_context_with(&scope, ON);
            let offered = ReadFile.offered_parameters(&on);
            assert!(offered["properties"].get("paths").is_some());
            assert!(offered["required"].as_array().is_none_or(Vec::is_empty));
            assert!(ReadFile.description(&on).contains("`paths`"));
            assert!(ReadFile.repeatable(&json!({ "paths": ["a", "b"] })));
        }
    }
}
