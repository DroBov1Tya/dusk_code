//! `click`: presses the control at a point of the last screenshot, or clicks with the
//! user's pointer.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[cfg(test)]
use super::Desktop;
use super::{blocking, screen::Screen};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        WindowAccess, schema,
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
            "a click without the pointer is a single left one; right and double clicks \
             need mode system, which moves the user's pointer"
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
    /// The right button instead of the left.
    #[serde(default)]
    right: bool,
    /// A double click.
    #[serde(default)]
    double: bool,
    /// Independent (default) activates an accessible control with AXPress without
    /// moving the pointer. System posts a real click and moves the user's pointer.
    #[serde(default)]
    mode: PointerMode,
}

impl Tool for Click {
    computer_tool!("click");

    fn description(&self, _: &ToolContext) -> String {
        if cfg!(target_os = "macos") {
            "Click at x, y of the newest screenshot. A selected window session uses app-scoped \
             Accessibility, even when another app covers the window. AXPress can bring the target \
             app to the foreground. By default it presses the control there \
             through Accessibility and leaves the user's pointer alone: a single left click \
             on a button, link or checkbox, which may not focus a field or the app. A control \
             that cannot be pressed so, a window that moved since the screenshot, or an \
             ambiguous window match is an error. Mode system takes the user's own pointer, \
             for right and double clicks outside a window session: use it only when the user \
             asked in so many words to use their mouse; it asks them first."
        } else {
            "Click at x, y of the newest screenshot, left by default, right or double when \
             asked. It moves the user's pointer there, so use it only when the user asked for \
             a click, and it asks them first."
        }
        .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<ClickArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: ClickArguments = parse_arguments(arguments)?;
            let count = if arguments.double { 2 } else { 1 };
            arguments.mode.validate_click(arguments.right, count)?;
            let action = match arguments.mode {
                PointerMode::Independent => format!(
                    "activate the control at {}, {} without moving the pointer",
                    arguments.x, arguments.y
                ),
                PointerMode::System => format!(
                    "move the user's pointer and click at {}, {}",
                    arguments.x, arguments.y
                ),
            };
            let scope = self.0.scope();
            match (&scope, arguments.mode) {
                (Some((id, label)), PointerMode::Independent) => {
                    context
                        .approve_window(
                            Some(WindowAccess::Activate(id.clone())),
                            &format!(
                                "{action} in {label}; the target app may come to the foreground"
                            ),
                        )
                        .await?
                }
                (Some(_), PointerMode::System) => {
                    anyhow::bail!("system pointer is disabled in a window session")
                }
                (None, PointerMode::Independent) => context.approve_screen(&action, false).await?,
                (None, PointerMode::System) => context.approve_system_pointer(&action).await?,
            }
            let screen = Arc::clone(&self.0);
            blocking(move || {
                screen.click(
                    arguments.x,
                    arguments.y,
                    arguments.right,
                    count,
                    arguments.mode,
                    scope.as_ref().map(|(id, _)| id.as_str()),
                )
            })
            .await?;
            match arguments.mode {
                PointerMode::Independent => Ok(format!(
                    "pressed the control at {}, {}; the keyboard focus may not have moved",
                    arguments.x, arguments.y
                )),
                PointerMode::System => Ok(format!(
                    "clicked at {}, {} with the pointer",
                    arguments.x, arguments.y
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(target_os = "macos")]
    #[test]
    fn existing_click_arguments_default_to_independent_activation() {
        let args: ClickArguments = parse_arguments(json!({"x": 12, "y": 34})).unwrap();
        assert_eq!(args.mode, PointerMode::Independent);
        assert!(!args.right && !args.double);
        assert!(args.mode.validate_click(args.right, 1).is_ok());
    }
    #[test]
    fn pointer_movement_requires_explicit_system_mode() {
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
        let args: ClickArguments = parse_arguments(
            json!({"x": 12, "y": 34, "right": true, "double": true, "mode": "system"}),
        )
        .unwrap();
        assert_eq!(args.mode, PointerMode::System);
        assert!(args.right && args.double);
        assert!(
            parse_arguments::<ClickArguments>(json!({"x": 12, "y": 34, "mode": "auto"})).is_err()
        );
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
        assert!(
            Desktop
                .click(at, false, 2, PointerMode::Independent, None)
                .is_err()
        );
    }
}
