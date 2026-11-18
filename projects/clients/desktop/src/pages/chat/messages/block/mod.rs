//! A block the model showed with `show`, as its own row of the transcript: a flow, a list
//! of checks or reply buttons. Until each kind has its own drawing it shows as its plain
//! text in a card headed "from the agent".
//!
//! Every text in a block is a `Line` the model wrote: drawn as it is, never as markdown or
//! a link. A block has no button, no fill of the accent and no focus stop, so it cannot pass
//! for a control of the app; the reply buttons of `Choices` are pills, never filled.

use dusk_protocol::thread::Block;
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// Draws the block at item `at` of the chat `shown`. The workspace is for the reply
/// buttons, which send a message and must know whether the chat moved on.
pub fn render(
    _ws: &Workspace,
    shown: &str,
    at: usize,
    block: &Block,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let kind = match block {
        Block::Flow(_) => "Flow",
        Block::Checks(_) => "Checks",
        Block::Choices(_) => "Choices",
    };
    let text = block.text();
    // The first line of the text is the heading, which the head already names.
    let body = text.split_once('\n').map_or("", |(_, rest)| rest);
    let muted = cx.theme().muted_foreground;
    widgets::card(cx)
        .id(SharedString::from(format!("block-{shown}-{at}")))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap_3()
                .child(div().small().font_weight(FontWeight::MEDIUM).child(kind))
                .child(div().meta().text_color(muted).child("from the agent")),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .small()
                .children(body.lines().map(|line| div().child(line.to_owned()))),
        )
        .into_any_element()
}
