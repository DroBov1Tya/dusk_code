//! The providers on the Model page: a card for each one connected, with what it answered and
//! buttons to check it, connect it anew or forget it, and a card that connects another one,
//! where it can be checked before it is saved. Keys are typed masked and go to the core only.

mod check;
mod confirm;
mod connect;
mod list;
mod state;

pub use confirm::confirm_remove_all;
pub use state::State;

use gpui_kit::{component::ActiveTheme, *};

use crate::{ui::Type, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .pt_4()
        .child(list::render(ws, cx))
        .child(connect::render(ws, cx))
}

/// A card: the one surface the page is built of.
fn card(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
}

/// A field's label above its box.
fn labeled(label: &'static str, hint: &'static str, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div().flex().flex_col().gap_1().child(
        div()
            .flex()
            .flex_row()
            .items_baseline()
            .gap_2()
            .child(div().small().font_weight(FontWeight::MEDIUM).child(label))
            .child(div().meta().text_color(muted).child(hint)),
    )
}
