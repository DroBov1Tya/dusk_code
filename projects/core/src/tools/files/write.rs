//! `write_file`: creates or overwrites a file, with its parent directories.

use anyhow::Context;
use tokio::fs;

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

use super::{skill_save, with_skill_save, write_text};

pub struct WriteFile;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteFileArguments {
    /// File to write.
    path: String,
    /// The whole new content.
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
        schema::of::<WriteFileArguments>()
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
            let saved = skill_save(scope, &path, &arguments.content).await;
            write_text(&path, arguments.content).await?;
            Ok(with_skill_save(
                format!(
                    "wrote {bytes} bytes to {}",
                    printable(&path.to_string_lossy())
                ),
                saved,
            ))
        })
    }
}
