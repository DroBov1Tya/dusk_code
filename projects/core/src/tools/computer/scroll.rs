//! `scroll`: the content under a point of the last screenshot, with the user's pointer.

use std::sync::Arc;

use anyhow::ensure;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, blocking, screen::Screen};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        schema,
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
            self.0.require_unscoped()?;
            let arguments: ScrollArguments = parse_arguments(arguments)?;
            let bounded = |pixels: i32| pixels.clamp(-MAX_SCROLL_PIXELS, MAX_SCROLL_PIXELS);
            let (down, right) = (bounded(arguments.down), bounded(arguments.right));
            ensure!(down != 0 || right != 0, "say how far to scroll");
            context
                .approve_system_pointer("move the user's pointer and scroll")
                .await?;
            let screen = Arc::clone(&self.0);
            blocking(move || {
                screen.unscoped(|| {
                    let at = screen.point(arguments.x, arguments.y)?;
                    screen.mark(at);
                    Desktop.scroll(at, down, right)
                })
            })
            .await?;
            Ok(format!("scrolled {down} down and {right} right"))
        })
    }
}
