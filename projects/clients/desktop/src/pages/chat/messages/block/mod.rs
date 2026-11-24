//! A block the model showed with `show`, as its own row of the transcript: a flow (`flow`),
//! a list of checks (`checks`), reply buttons (`choices`) or a diagram (`diagram`), with
//! `glyph` for the small drawings.
//!
//! Every text in a block is a `Line` the model wrote: drawn as it is, never as markdown or
//! a link. A block has no button, no fill of the accent and no focus stop, so it cannot pass
//! for a control of the app; the reply buttons of `Choices` are pills, never filled.

use dusk_protocol::thread::Block;
use gpui_kit::*;

use crate::workspace::Workspace;

mod checks;
mod choices;
mod diagram;
mod flow;
mod glyph;

pub use choices::{Pick, pills};

/// Draws the block at item `at` of the chat `shown`. The workspace is for the reply
/// buttons, which send a message and must know whether the chat moved on.
pub fn render(
    ws: &Workspace,
    shown: &str,
    at: usize,
    block: &Block,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    match block {
        Block::Flow(flow) => flow::render(ws, shown, at, flow, cx),
        Block::Checks(checks) => checks::render(checks, cx),
        Block::Choices(choices) => choices::render(ws, shown, at, choices, cx),
        Block::Diagram(diagram) => diagram::render(ws, shown, at, diagram, cx),
    }
}
