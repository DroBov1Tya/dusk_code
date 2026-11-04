//! `type_text`: text into whatever has the keyboard focus.

use std::sync::Arc;

use anyhow::ensure;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, Screen, shared::blocking};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

/// Text typed in one call at most.
const MAX_TYPED_CHARS: usize = 2000;
/// Of the text to type, what the approval request shows.
const SHOWN_CHARS: usize = 200;

pub struct TypeText(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TypeTextArguments {
    /// What to type into the focused field, as it is.
    text: String,
}

impl Tool for TypeText {
    computer_tool!("type_text");

    fn description(&self, _: &ToolContext) -> String {
        format!(
            "Type text into whatever has the shared system keyboard focus, as a person \
             would. Independent AXPress does not guarantee focus: verify the focused \
             field and app first. Up to {MAX_TYPED_CHARS} characters. For Enter, shortcuts and \
             other keys use press_keys."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<TypeTextArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            self.0.require_unscoped()?;
            let arguments: TypeTextArguments = parse_arguments(arguments)?;
            let count = arguments.text.chars().count();
            ensure!(count > 0, "the text is empty");
            ensure!(
                count <= MAX_TYPED_CHARS,
                "type at most {MAX_TYPED_CHARS} characters at once"
            );
            // The user sees what will be typed, since it can run anything a shell can.
            let shown: String = arguments.text.chars().take(SHOWN_CHARS).collect();
            let more = if count > SHOWN_CHARS { "..." } else { "" };
            let action = format!("type \"{}{more}\"", shown.escape_debug());
            context.approve_screen(&action, true).await?;
            let screen = Arc::clone(&self.0);
            blocking(move || screen.unscoped(|| Desktop.type_text(&arguments.text))).await?;
            Desktop.touch();
            Ok(format!("typed {count} characters"))
        })
    }
}
