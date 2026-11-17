//! `scroll`: the content under a point of the last screenshot, with the user's pointer.

use std::sync::Arc;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, screen::Screen, step::Step};
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

pub struct Scroll(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScrollArguments {
    /// Where to scroll, in the newest screenshot.
    x: f64,
    y: f64,
    /// Pixels; positive scrolls the content down.
    #[serde(default)]
    down: i32,
    /// Pixels; positive scrolls the content right.
    #[serde(default)]
    right: i32,
}

impl ScrollArguments {
    /// Bounded before anything is asked or done.
    fn parse(arguments: Value) -> Result<Self> {
        let mut arguments: Self = parse_arguments(arguments)?;
        let bounded = |pixels: i32| pixels.clamp(-MAX_SCROLL_PIXELS, MAX_SCROLL_PIXELS);
        arguments.down = bounded(arguments.down);
        arguments.right = bounded(arguments.right);
        ensure!(
            arguments.down != 0 || arguments.right != 0,
            "say how far to scroll"
        );
        Ok(arguments)
    }
}

impl Scroll {
    /// Checked before anything is asked or done, so a batch can check every step first.
    pub(super) fn step(screen: &Arc<Screen>, arguments: Value) -> Result<Step> {
        screen.require_unscoped()?;
        let ScrollArguments { x, y, down, right } = ScrollArguments::parse(arguments)?;
        let request = Request::SystemPointer {
            action: format!(
                "move the user's pointer to {x}, {y} and scroll {down} down, {right} right"
            ),
        };
        let screen = Arc::clone(screen);
        Ok(Step::new(request, move || {
            screen.unscoped(|| {
                let at = screen.point(x, y)?;
                screen.mark(at);
                Desktop.scroll(at, down, right)
            })?;
            Ok(ToolResult::text(format!(
                "scrolled {down} down and {right} right"
            )))
        }))
    }
}

impl Tool for Scroll {
    computer_tool!("scroll");

    fn description(&self, _: &ToolContext) -> String {
        "Scroll what is under x, y of the newest screenshot by pixels down or right; \
         negative goes up or left. It moves the user's pointer there, so use it only when \
         the user asked for it, and it asks them first."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<ScrollArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let step = Self::step(&self.0, arguments)?;
            Ok(step.ask_and_run(&context).await?.content)
        })
    }
}
