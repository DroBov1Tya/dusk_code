//! `edit_file`: replaces one exact occurrence of a text in a file.

use anyhow::{bail, ensure};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    tools::{
        builtin::{parse_arguments, printable},
        schema,
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
        schema::of::<EditFileArguments>()
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
            let saved = skill_save(scope, &path, &edited).await;
            write_text(&path, edited).await?;
            Ok(with_skill_save(
                format!("edited {}", printable(&path.to_string_lossy())),
                saved,
            ))
        })
    }
}
