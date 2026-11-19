//! `click`: presses the control at a point of the last screenshot, or clicks with the
//! user's pointer.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[cfg(test)]
use super::Desktop;
use super::{physical_input::USE_PHYSICAL_INPUT, screen::Screen, step::Step};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        Request, ToolResult, WindowAccess, schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

/// How a click reaches the screen. A click never falls back from one to the other.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointerMode {
    /// Presses the control through Accessibility; the user's pointer stays put. Only
    /// macOS has it.
    #[cfg_attr(target_os = "macos", default)]
    Independent,
    /// Posts mouse events, moving the user's pointer.
    #[cfg_attr(not(target_os = "macos"), default)]
    System,
}

impl PointerMode {
    pub fn validate_click(self, right: bool, count: i64) -> Result<()> {
        ensure!((1..=2).contains(&count), "a click count must be one or two");
        ensure!(
            self == Self::System || (!right && count == 1),
            "a press through Accessibility is a single left one; {}",
            super::physical_input::USE_PHYSICAL_INPUT
        );
        Ok(())
    }
}

pub struct Click(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClickArguments {
    x: f64,
    y: f64,
    /// Fields of the old click, accepted by the check and hidden from the model, so a
    /// saved call that asks for the pointer is refused with the way forward.
    #[serde(default)]
    right: bool,
    #[serde(default)]
    double: bool,
    #[serde(default)]
    mode: Option<PointerMode>,
}

impl ClickArguments {
    /// Bounded before anything is asked or done. A saved call of the old click that asked
    /// for the real pointer is refused with where that went.
    fn parse(arguments: Value) -> Result<Self> {
        let arguments: Self = parse_arguments(arguments)?;
        let pointer =
            arguments.right || arguments.double || arguments.mode == Some(PointerMode::System);
        ensure!(!pointer, USE_PHYSICAL_INPUT);
        Ok(arguments)
    }

    /// The one way this desktop clicks: through Accessibility on macOS, the pointer on
    /// Linux, which has no press without it.
    fn mode(&self) -> PointerMode {
        PointerMode::default()
    }
}

impl Click {
    /// Checked before anything is asked or done, so a batch can check every step first.
    pub(super) fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        let arguments = ClickArguments::parse(arguments)?;
        let (x, y, mode) = (arguments.x, arguments.y, arguments.mode());
        let action = match mode {
            PointerMode::Independent => {
                format!("activate the control at {x}, {y} without moving the pointer")
            }
            PointerMode::System => format!("move the user's pointer and click at {x}, {y}"),
        };
        let scope = screen.scope();
        let request = match (&scope, mode) {
            (Some((id, label)), PointerMode::Independent) => Request::Window {
                grant: Some(WindowAccess::Activate(id.clone())),
                action: format!("{action} in {label}; the target app may come to the foreground"),
            },
            (Some(_), PointerMode::System) => {
                anyhow::bail!("system pointer is disabled in a window session")
            }
            (None, PointerMode::Independent) => Request::Screen { action },
            (None, PointerMode::System) => Request::SystemPointer { action },
        };
        let screen = Arc::clone(screen);
        Ok(Step::new(request, move || {
            let expected = scope.as_ref().map(|(id, _)| id.as_str());
            screen.click(x, y, false, 1, mode, expected)?;
            Ok(ToolResult::text(match mode {
                PointerMode::Independent => format!(
                    "pressed the control at {x}, {y}; the keyboard focus may not have moved"
                ),
                PointerMode::System => format!("clicked at {x}, {y} with the pointer"),
            }))
        }))
    }
}

impl Tool for Click {
    computer_tool!("click");

    fn description(&self, _: &ToolContext) -> String {
        if cfg!(target_os = "macos") {
            "Press the control at x, y of the newest screenshot through Accessibility; the \
             user's pointer stays put. A single left press on a button, link, checkbox or \
             menu item; it may not focus a text field, and may bring the app forward. A \
             control that cannot be pressed, or a window that moved since the screenshot, \
             is an error."
        } else {
            "Left click at x, y of the newest screenshot. It moves the user's pointer there \
             and asks them first."
        }
        .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<ClickArguments>()
    }

    fn offered_parameters(&self, _: &ToolContext) -> Value {
        offered()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let step = Self::step(&self.0, arguments)?;
            Ok(step.ask_and_run(&context).await?.content)
        })
    }
}

/// The schema without the old fields, which only a saved call still sends.
fn offered() -> Value {
    let mut schema = schema::of::<ClickArguments>();
    if let Some(properties) = schema["properties"].as_object_mut() {
        for old in OLD_FIELDS {
            properties.remove(old);
        }
    }
    schema
}

const OLD_FIELDS: [&str; 3] = ["right", "double", "mode"];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plain_click_parses() {
        let args = ClickArguments::parse(json!({"x": 12, "y": 34})).unwrap();
        assert_eq!((args.x, args.y), (12.0, 34.0));
        assert!(ClickArguments::parse(json!({"x": 1, "y": 1, "mode": "independent"})).is_ok());
    }

    /// Saved calls of the old click that took the real pointer are refused, naming the tool.
    #[test]
    fn the_old_pointer_parameter_is_refused_with_the_way_forward() {
        for old in [
            json!({"x": 1, "y": 1, "mode": "system"}),
            json!({"x": 1, "y": 1, "right": true}),
            json!({"x": 1, "y": 1, "double": true}),
        ] {
            let Err(error) = ClickArguments::parse(old) else {
                panic!("an old pointer call must be refused");
            };
            let error = error.to_string();
            assert!(error.contains("physical_input"), "{error}");
        }
        assert!(ClickArguments::parse(json!({"x": 1, "y": 1, "mode": "auto"})).is_err());
        let schema = offered();
        for old in OLD_FIELDS {
            assert!(schema["properties"].get(old).is_none(), "{schema}");
        }
        assert!(schema["properties"].get("x").is_some(), "{schema}");
    }

    #[test]
    fn pointer_movement_requires_system_mode() {
        for (right, count) in [(true, 1), (false, 2), (true, 2)] {
            assert!(
                PointerMode::Independent
                    .validate_click(right, count)
                    .is_err()
            );
            assert!(PointerMode::System.validate_click(right, count).is_ok());
        }
        for count in [0, 3, -1] {
            assert!(PointerMode::System.validate_click(false, count).is_err());
        }
    }

    #[test]
    fn unsupported_backend_actions_fail_before_permission_or_event_access() {
        let at = super::super::Point { x: 0.0, y: 0.0 };
        assert!(
            Desktop
                .click(at, true, 1, PointerMode::Independent, None)
                .unwrap_err()
                .to_string()
                .contains("single left")
        );
    }
}
