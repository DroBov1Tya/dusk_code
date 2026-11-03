//! `read_file`: a UTF-8 text file, up to 1 MiB.

use serde_json::Value;

use crate::{
    BoxFuture,
    tools::{
        schema,
        schema::parse_arguments,
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

    fn description(&self, context: &ToolContext) -> String {
        let scope = &*context.scope;
        format!(
            "Read a UTF-8 text file up to 1 MiB. Relative paths resolve against {}.",
            scope.cwd().display()
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<PathArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: PathArguments = parse_arguments(arguments)?;
            read_text(&context.resolve(&arguments.path, false).await?).await
        })
    }
}
