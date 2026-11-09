//! `edit_file`: replaces one exact occurrence of a text in a file.

use anyhow::{Result, bail, ensure};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::util::fs::printable;
use crate::{
    BoxFuture,
    tools::{
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::{read_text, skill_save, with_skill_save, write_text};

pub struct EditFile;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditFileArguments {
    /// File to edit.
    path: String,
    /// Exact text to replace, occurring once in the file.
    old_text: String,
    /// Replacement text.
    new_text: String,
}

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, context: &ToolContext) -> String {
        let scope = &*context.scope;
        format!(
            "Replace one exact occurrence of old_text with new_text in a file. old_text must \
             match exactly once; include surrounding lines to make it unique. \
             Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<EditFileArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let scope = &*context.scope;
            let arguments: EditFileArguments = parse_arguments(arguments)?;
            ensure!(!arguments.old_text.is_empty(), "old_text is empty");
            let path = context.resolve(&arguments.path, true).await?;
            let text = read_text(&path).await?;
            let (old_text, new_text) =
                line_endings_of(&text, arguments.old_text, arguments.new_text);
            let edited = replace_once(&text, &old_text, &new_text)?;
            let saved = skill_save(scope, &path, &edited).await;
            write_text(&path, edited).await?;
            Ok(with_skill_save(
                format!("edited {}", printable(&path.to_string_lossy())),
                saved,
            ))
        })
    }
}

/// A file with CRLF line ends gets the edit's LF text in CRLF, so the match does not
/// fail on line ends the model cannot see.
fn line_endings_of(text: &str, old_text: String, new_text: String) -> (String, String) {
    if text.contains("\r\n") && !old_text.contains('\r') {
        (
            old_text.replace('\n', "\r\n"),
            new_text.replace('\n', "\r\n"),
        )
    } else {
        (old_text, new_text)
    }
}

/// Replaces the one occurrence; a miss says what the file holds near the text, so the
/// model can fix the edit without reading the file again.
fn replace_once(text: &str, old_text: &str, new_text: &str) -> Result<String> {
    let starts: Vec<usize> = text.match_indices(old_text).map(|(at, _)| at).collect();
    match starts.as_slice() {
        [_] => Ok(text.replacen(old_text, new_text, 1)),
        [] if !new_text.is_empty() && text.contains(new_text) => {
            bail!("old_text not found, but new_text is already in the file: the edit may be done")
        }
        [] => bail!("old_text not found.{}", nearest_lines(text, old_text)),
        many => {
            let lines: Vec<String> = many
                .iter()
                .take(5)
                .map(|at| line_of(text, *at).to_string())
                .collect();
            bail!(
                "old_text matches {} times (lines {}); include surrounding lines to make it unique",
                many.len(),
                lines.join(", ")
            )
        }
    }
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Up to two places whose line equals the edit's first non-blank line once spaces are
/// trimmed, each with a few lines around it, numbered.
fn nearest_lines(text: &str, old_text: &str) -> String {
    const AROUND: usize = 4;
    let Some(first) = old_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
    else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let found: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == first)
        .map(|(index, _)| index)
        .take(2)
        .collect();
    if found.is_empty() {
        return " Its first line is not in the file either; read the file again.".to_owned();
    }
    let mut hint = String::from(" The file near its first line:");
    for index in found {
        let end = (index + old_text.lines().count() + AROUND).min(lines.len());
        let start = index.saturating_sub(AROUND);
        for (number, line) in lines[start..end].iter().enumerate() {
            hint.push_str(&format!("\n{:>5}  {line}", start + number + 1));
        }
        hint.push_str("\n  ...");
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_match_is_replaced() {
        assert_eq!(replace_once("a b c", "b", "x").unwrap(), "a x c");
    }

    #[test]
    fn a_miss_shows_the_lines_near_the_first_line() {
        let text = "fn main() {\n    let x = 1;\n    run(x);\n}\n";
        let error = replace_once(text, "let x = 1;\n  run(x);", "y")
            .unwrap_err()
            .to_string();
        assert!(error.contains("    2      let x = 1;"), "{error}");
        assert!(error.contains("    3      run(x);"), "{error}");
    }

    #[test]
    fn an_edit_already_made_says_so() {
        let error = replace_once("new", "old", "new").unwrap_err().to_string();
        assert!(error.contains("already in the file"));
    }

    #[test]
    fn many_matches_name_their_lines() {
        let error = replace_once("a\nb\na\n", "a", "x").unwrap_err().to_string();
        assert!(error.contains("2 times (lines 1, 3)"), "{error}");
    }

    #[test]
    fn crlf_files_take_lf_edits() {
        let (old, new) = line_endings_of("a\r\nb\r\n", "a\nb".into(), "c\nd".into());
        assert_eq!(
            replace_once("a\r\nb\r\n", &old, &new).unwrap(),
            "c\r\nd\r\n"
        );
    }
}
