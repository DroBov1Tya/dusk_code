//! `delete_file`: removes a file, never a directory.

use anyhow::{Context, ensure};
use tokio::fs;

use serde_json::{Value, json};

use crate::{
    BoxFuture,
    tools::{
        builtin::{parse_arguments, printable},
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::PathArguments;

pub struct DeleteFile;

impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
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
