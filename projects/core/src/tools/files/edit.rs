//! `edit_file`: replaces one exact occurrence of a text in a file.

use anyhow::{bail, ensure};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    tools::{
        builtin::{parse_arguments, printable},
        tool::{Tool, ToolContext, ToolKind},
    },
};

use super::{read_text, write_text};

pub struct EditFile;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditFileArguments {
    path: String,
    old_text: String,
    new_text: String,
}

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
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
