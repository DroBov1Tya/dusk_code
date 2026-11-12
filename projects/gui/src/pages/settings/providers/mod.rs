//! The providers on the Model page: a card for each one connected, with what it answered and
//! buttons to check it, connect it anew or forget it, and a card that connects another one,
//! where it can be checked before it is saved. Keys are typed masked and go to the core only.

mod check;
mod confirm;
mod connect;
mod list;
mod state;

pub use check::Check;
pub use confirm::confirm_remove_all;
pub use state::State;

use gpui_kit::*;

use crate::workspace::Workspace;

/// The card that connects a provider, alone: the model menu opens it in a window.
pub fn connect_card(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    connect::render(ws, cx)
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .pt_4()
        .child(list::render(ws, cx))
        .child(connect::render(ws, cx))
}
