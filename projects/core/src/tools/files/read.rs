//! `read_file`: a UTF-8 text file, a window of lines at a time, so one read cannot
//! flood the context.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    tools::{
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::read_text;

/// Lines one read returns when the model does not ask for fewer.
const MAX_LINES: usize = 2000;
/// Bytes one read returns at most; the window ends at the last whole line before it.
const MAX_BYTES: usize = 50 * 1024;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    /// The file.
    path: String,
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
             file, find the place with search_files first and read only that part. \
             Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<ReadArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: ReadArguments = parse_arguments(arguments)?;
            let text = read_text(&context.resolve(&arguments.path, false).await?).await?;
            Ok(window(&text, arguments.offset, arguments.limit))
        })
    }
}

/// The lines from `offset` (1-based), at most `limit` of them and `MAX_BYTES` in all,
/// with a note when the file goes on. A whole small file comes back unchanged.
fn window(text: &str, offset: Option<usize>, limit: Option<usize>) -> String {
    let total = text.lines().count();
    let first = offset.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(MAX_LINES).clamp(1, MAX_LINES);
    if first == 1 && total <= limit && text.len() <= MAX_BYTES {
        return text.to_owned();
    }
    if first > total {
        return format!("[the file has {total} lines; offset {first} is past its end]");
    }
    let mut out = String::new();
    let mut last = first - 1;
    for line in text.lines().skip(first - 1).take(limit) {
        if out.len() + line.len() + 1 > MAX_BYTES && last >= first {
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
}
