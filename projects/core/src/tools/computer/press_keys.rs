//! `press_keys`: a key or a shortcut in whatever has the keyboard focus.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, Screen, screen::keys, shared::blocking};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

pub struct PressKeys(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PressKeysArguments {
    /// One key with its modifiers, such as `enter`, `cmd+t`, `cmd+shift+4` or `down`.
    keys: String,
}

impl Tool for PressKeys {
    computer_tool!("press_keys");

    fn description(&self, _: &ToolContext) -> String {
        "Press a key or a shortcut in whatever has the keyboard focus: a letter, digit, \
         enter, tab, space, esc, delete, the arrows, home, end, pageup, pagedown or f1 to \
         f12, with cmd, shift, ctrl and alt joined by +."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<PressKeysArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            self.0.require_unscoped()?;
            let arguments: PressKeysArguments = parse_arguments(arguments)?;
            let combo = keys(&arguments.keys)?;
            context
                .approve_screen(&format!("press {}", arguments.keys), true)
                .await?;
            let screen = Arc::clone(&self.0);
            blocking(move || screen.unscoped(|| Desktop.press(&combo))).await?;
            Desktop.touch();
            Ok(format!("pressed {}", arguments.keys))
        })
    }
}
