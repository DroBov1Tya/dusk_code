//! File tools: read, write, edit and delete files inside the tool scope.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;

use super::{
    builtin::{parse_arguments, printable, read_regular, write_regular},
    tool::{Tool, ToolContext},
};
use crate::BoxFuture;

/// Larger files would not fit the result limit anyway.
const MAX_READ_BYTES: u64 = 1024 * 1024;

pub struct ReadFile;
pub struct WriteFile;
pub struct EditFile;
pub struct DeleteFile;

/// For tools that take only a path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArguments {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFileArguments {
    path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditFileArguments {
    path: String,
    old_text: String,
    new_text: String,
}

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let scope = context.scope;
        format!(
            "Read a UTF-8 text file up to 1 MiB. Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to read." }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope;
        Box::pin(async move {
            let arguments: PathArguments = parse_arguments(arguments)?;
            read_text(&scope.resolve(&arguments.path)?).await
        })
    }
}

impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let scope = context.scope;
        format!(
            "Create or overwrite a file with the given content, creating parent directories. \
             Relative paths resolve against {}; scratch files belong in {}.",
            scope.cwd().display(),
            scope.workspace().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to write." },
                "content": { "type": "string", "description": "The whole new content." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope;
        Box::pin(async move {
            let arguments: WriteFileArguments = parse_arguments(arguments)?;
            let path = scope.resolve(&arguments.path)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .context("cannot create parent directories")?;
            }
            let bytes = arguments.content.len();
            write_text(&path, arguments.content).await?;
            Ok(format!(
                "wrote {bytes} bytes to {}",
                printable(&path.to_string_lossy())
            ))
        })
    }
}

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let scope = context.scope;
        format!(
            "Replace one exact occurrence of old_text with new_text in a file. old_text must \
             match exactly once; include surrounding lines to make it unique. \
             Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to edit." },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to replace, occurring once in the file."
                },
                "new_text": { "type": "string", "description": "Replacement text." }
            },
            "required": ["path", "old_text", "new_text"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope;
        Box::pin(async move {
            let arguments: EditFileArguments = parse_arguments(arguments)?;
            ensure!(!arguments.old_text.is_empty(), "old_text is empty");
            let path = scope.resolve(&arguments.path)?;
            let text = read_text(&path).await?;
            let matches = text.matches(&arguments.old_text).count();
            if matches != 1 {
                bail!("old_text must match exactly once, found {matches} matches");
            }
            let edited = text.replacen(&arguments.old_text, &arguments.new_text, 1);
            write_text(&path, edited).await?;
            Ok(format!("edited {}", printable(&path.to_string_lossy())))
        })
    }
}

impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete_file"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let scope = context.scope;
        format!(
            "Delete a file. Directories are not deleted; use run_command for them. Relative \
             paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to delete." }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope;
        Box::pin(async move {
            let arguments: PathArguments = parse_arguments(arguments)?;
            let path = scope.resolve(&arguments.path)?;
            let metadata = fs::symlink_metadata(&path).await.context("no such file")?;
            ensure!(metadata.is_file(), "not a file");
            fs::remove_file(&path).await.context("cannot delete file")?;
            Ok(format!("deleted {}", printable(&path.to_string_lossy())))
        })
    }
}

async fn read_text(path: &Path) -> Result<String> {
    let path = path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || read_regular(&path, MAX_READ_BYTES))
        .await
        .context("cannot read file")??;
    String::from_utf8(bytes).map_err(|_| anyhow!("file is not valid UTF-8 text"))
}

async fn write_text(path: &Path, text: String) -> Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || write_regular(&path, text.as_bytes()))
        .await
        .context("cannot write file")?
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{
        testing::{temp_dir, tool_context},
        tools::ToolScope,
    };

    fn scope(name: &str) -> (ToolScope, PathBuf) {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        (ToolScope::new(&cwd, &workspace).unwrap(), root)
    }

    #[tokio::test]
    async fn reads_a_file() {
        let (scope, _) = scope("files_read");
        fs::write(scope.cwd().join("a.txt"), "hello").unwrap();
        let output = ReadFile
            .call(json!({ "path": "a.txt" }), tool_context(&scope))
            .await
            .unwrap();
        assert_eq!(output, "hello");
    }

    #[tokio::test]
    async fn refuses_large_and_binary_files() {
        let (scope, _) = scope("files_read_refuse");
        fs::write(scope.cwd().join("big"), vec![b'a'; 1024 * 1024 + 1]).unwrap();
        fs::write(scope.cwd().join("bin"), [0xff, 0xfe]).unwrap();
        for path in ["big", "bin"] {
            assert!(
                ReadFile
                    .call(json!({ "path": path }), tool_context(&scope))
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_scope() {
        let (scope, root) = scope("files_outside");
        fs::write(root.join("secret"), "x").unwrap();
        let outside = "../secret";
        assert!(
            ReadFile
                .call(json!({ "path": outside }), tool_context(&scope))
                .await
                .is_err()
        );
        let write = json!({ "path": outside, "content": "y" });
        assert!(WriteFile.call(write, tool_context(&scope)).await.is_err());
        let edit = json!({ "path": outside, "old_text": "x", "new_text": "y" });
        assert!(EditFile.call(edit, tool_context(&scope)).await.is_err());
        assert_eq!(fs::read_to_string(root.join("secret")).unwrap(), "x");
    }

    #[tokio::test]
    async fn rejects_unknown_and_missing_fields() {
        let (scope, _) = scope("files_arguments");
        let unknown = json!({ "path": "a", "extra": 1 });
        assert!(ReadFile.call(unknown, tool_context(&scope)).await.is_err());
        assert!(
            WriteFile
                .call(json!({ "path": "a" }), tool_context(&scope))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn writes_with_parent_directories() {
        let (scope, _) = scope("files_write");
        let arguments = json!({ "path": "x/y/z.txt", "content": "abc" });
        let output = WriteFile
            .call(arguments, tool_context(&scope))
            .await
            .unwrap();
        assert!(output.starts_with("wrote 3 bytes to "), "{output}");
        let written = fs::read_to_string(scope.cwd().join("x/y/z.txt")).unwrap();
        assert_eq!(written, "abc");
    }

    #[tokio::test]
    async fn deletes_files_only() {
        let (scope, _) = scope("files_delete");
        fs::write(scope.cwd().join("a.txt"), "x").unwrap();
        fs::create_dir(scope.cwd().join("dir")).unwrap();
        let delete = |path: &str| DeleteFile.call(json!({ "path": path }), tool_context(&scope));
        delete("a.txt").await.unwrap();
        assert!(!scope.cwd().join("a.txt").exists());
        assert!(delete("dir").await.is_err());
        assert!(delete("../outside").await.is_err());
    }

    #[tokio::test]
    async fn edits_a_single_match() {
        let (scope, _) = scope("files_edit");
        let path = scope.cwd().join("a.txt");
        fs::write(&path, "one two three").unwrap();
        let arguments = json!({ "path": "a.txt", "old_text": "two", "new_text": "2" });
        EditFile
            .call(arguments, tool_context(&scope))
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one 2 three");
    }

    #[tokio::test]
    async fn edit_requires_exactly_one_match() {
        let (scope, _) = scope("files_edit_matches");
        let path = scope.cwd().join("a.txt");
        fs::write(&path, "x y x").unwrap();
        for (old_text, expected) in [("z", "found 0 matches"), ("x", "found 2 matches")] {
            let arguments = json!({ "path": "a.txt", "old_text": old_text, "new_text": "q" });
            let error = EditFile
                .call(arguments, tool_context(&scope))
                .await
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "x y x");
    }
}
