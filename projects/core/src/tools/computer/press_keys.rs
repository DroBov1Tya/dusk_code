//! `press_keys`: a key or a shortcut in whatever has the keyboard focus.

use std::sync::Arc;

use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, Keys, Screen, screen::keys, step::Step};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        Request, ToolResult, schema,
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

impl PressKeysArguments {
    /// Bounded before anything is asked or done.
    fn parse(arguments: Value) -> Result<Keys> {
        let arguments: Self = parse_arguments(arguments)?;
        keys(&arguments.keys)
    }
}

impl PressKeys {
    /// Checked before anything is asked or done, so a batch can check every step first.
    pub(super) fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        screen.require_unscoped()?;
        let combo = PressKeysArguments::parse(arguments)?;
        let keys = combo.shown();
        let request = Request::Keyboard {
            action: format!("press {keys}"),
        };
        let screen = Arc::clone(screen);
        Ok(Step::new(request, move || {
            screen.unscoped(|| Desktop.press(&combo))?;
            Desktop.touch();
            Ok(ToolResult::text(format!("pressed {keys}")))
        }))
    }
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
            let step = Self::step(&self.0, arguments)?;
            Ok(step.ask_and_run(&context).await?.content)
        })
    }
}
