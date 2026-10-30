//! `write_file`: creates or overwrites a file, with its parent directories.

use anyhow::Context;
use tokio::fs;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    tools::{
        builtin::{parse_arguments, printable},
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::write_text;

pub struct WriteFile;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFileArguments {
    path: String,
    content: String,
}

impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
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
