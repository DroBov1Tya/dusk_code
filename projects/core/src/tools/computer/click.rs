//! `click`: presses the control at a point of the last screenshot, or clicks with the
//! user's pointer.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[cfg(test)]
use super::Desktop;
use super::{screen::Screen, step::Step};
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

impl ClickArguments {
    /// Bounded before anything is asked or done.
    fn parse(arguments: Value) -> Result<Self> {
        let arguments: Self = parse_arguments(arguments)?;
        arguments
            .mode
            .validate_click(arguments.right, arguments.count())?;
        Ok(arguments)
    }

    fn count(&self) -> i64 {
        if self.double { 2 } else { 1 }
    }
}

impl Click {
    /// Checked before anything is asked or done, so a batch can check every step first.
    pub(super) fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        let arguments = ClickArguments::parse(arguments)?;
        let (x, y, mode) = (arguments.x, arguments.y, arguments.mode);
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
            let count = arguments.count();
            let expected = scope.as_ref().map(|(id, _)| id.as_str());
            screen.click(x, y, arguments.right, count, mode, expected)?;
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
            let step = Self::step(&self.0, arguments)?;
            Ok(step.ask_and_run(&context).await?.content)
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
