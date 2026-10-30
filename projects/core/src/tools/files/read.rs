//! `read_file`: a UTF-8 text file, up to 1 MiB.

use serde_json::{Value, json};

use crate::{
    BoxFuture,
    tools::{
        builtin::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::{PathArguments, read_text};

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
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
