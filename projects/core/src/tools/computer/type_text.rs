//! `type_text`: text into whatever has the keyboard focus.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, Screen, step::Step};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        Request, ToolResult, schema,
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

impl TypeTextArguments {
    /// Bounded before anything is asked or done.
    fn parse(arguments: Value) -> Result<Self> {
        let arguments: Self = parse_arguments(arguments)?;
        let count = arguments.text.chars().count();
        ensure!(count > 0, "the text is empty");
        ensure!(
            count <= MAX_TYPED_CHARS,
            "type at most {MAX_TYPED_CHARS} characters at once"
        );
        Ok(arguments)
    }
}

impl TypeText {
    /// Checked before anything is asked or done, so a batch can check every step first.
    pub(super) fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        screen.require_unscoped()?;
        let TypeTextArguments { text } = TypeTextArguments::parse(arguments)?;
        let count = text.chars().count();
        // The user sees what will be typed, since it can run anything a shell can.
        let shown: String = text.chars().take(SHOWN_CHARS).collect();
        let more = if count > SHOWN_CHARS { "..." } else { "" };
        let request = Request::Keyboard {
            action: format!("type \"{}{more}\"", shown.escape_debug()),
        };
        let screen = Arc::clone(screen);
        Ok(Step::new(request, move || {
            screen.unscoped(|| Desktop.type_text(&text))?;
            Desktop.touch();
            Ok(ToolResult::text(format!("typed {count} characters")))
        }))
    }
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
            let step = Self::step(&self.0, arguments)?;
            Ok(step.ask_and_run(&context).await?.content)
        })
    }
}
