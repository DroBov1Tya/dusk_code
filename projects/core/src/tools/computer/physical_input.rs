//! `physical_input`: clicks and scrolls with the user's real pointer at a point of the
//! last screenshot. Off until the user switches it on in Settings; every use is asked
//! for under the chat's approval mode, apart from any yes for the screen.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, click::PointerMode, screen::Screen, step::Step};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        Request, ToolResult, schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

const MAX_SCROLL_PIXELS: i32 = 5000;

/// Why a press through Accessibility cannot do what was asked, and what to use instead.
pub const USE_PHYSICAL_INPUT: &str = "right and double clicks and the real pointer belong to \
     the physical_input tool; if it is not offered, the user has not switched it on (Settings, \
     Tools, physical_input), so press with click or ask them to switch it on";

pub struct PhysicalInput(pub Arc<Screen>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    Click,
    DoubleClick,
    RightClick,
    Scroll,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    /// The point in the newest screenshot.
    x: f64,
    y: f64,
    /// Scroll only: pixels, positive scrolls the content down.
    #[serde(default)]
    down: i32,
    /// Scroll only: pixels, positive scrolls the content right.
    #[serde(default)]
    right: i32,
}

impl Arguments {
    /// Bounded before anything is asked or done.
    fn parse(arguments: Value) -> Result<Self> {
        let mut arguments: Self = parse_arguments(arguments)?;
        let bounded = |pixels: i32| pixels.clamp(-MAX_SCROLL_PIXELS, MAX_SCROLL_PIXELS);
        arguments.down = bounded(arguments.down);
        arguments.right = bounded(arguments.right);
        if arguments.action == Action::Scroll {
            ensure!(
                arguments.down != 0 || arguments.right != 0,
                "say how far to scroll with down or right"
            );
        } else {
            ensure!(
                arguments.down == 0 && arguments.right == 0,
                "down and right are for scroll only"
            );
        }
        Ok(arguments)
    }
}

impl PhysicalInput {
    fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        screen.require_unscoped()?;
        let Arguments {
            action,
            x,
            y,
            down,
            right,
        } = Arguments::parse(arguments)?;
        let what = match action {
            Action::Click => "click".to_owned(),
            Action::DoubleClick => "double click".to_owned(),
            Action::RightClick => "right click".to_owned(),
            Action::Scroll => format!("scroll {down} down, {right} right"),
        };
        let request = Request::SystemPointer {
            action: format!("move your real pointer to {x}, {y} and {what}"),
        };
        let screen = Arc::clone(screen);
        Ok(Step::new(request, move || {
            match action {
                Action::Scroll => screen.unscoped(|| {
                    let at = screen.point(x, y)?;
                    screen.mark(at);
                    Desktop.scroll(at, down, right)
                })?,
                Action::Click | Action::DoubleClick | Action::RightClick => {
                    let count = if action == Action::DoubleClick { 2 } else { 1 };
                    let right = action == Action::RightClick;
                    screen.click(x, y, right, count, PointerMode::System, None)?;
                }
            }
            Ok(ToolResult::text(format!(
                "{what} at {x}, {y} with the real pointer"
            )))
        }))
    }
}

impl Tool for PhysicalInput {
    computer_tool!("physical_input");

    fn description(&self, _: &ToolContext) -> String {
        "Moves the user's real mouse: click, double_click, right_click or scroll at x, y of \
         the newest screenshot. Use only for what click cannot do, such as a canvas, a \
         right-click menu or scrolling; each use asks the user."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
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

    #[test]
    fn arguments_are_bounded_per_action() {
        assert!(Arguments::parse(json!({"action": "scroll", "x": 1, "y": 1})).is_err());
        assert!(Arguments::parse(json!({"action": "click", "x": 1, "y": 1, "down": 3})).is_err());
        let scroll =
            Arguments::parse(json!({"action": "scroll", "x": 1, "y": 1, "down": 99999})).unwrap();
        assert_eq!(scroll.down, MAX_SCROLL_PIXELS);
        assert!(Arguments::parse(json!({"action": "drag", "x": 1, "y": 1})).is_err());
    }
}
