//! Every chat in one list by its last activity, each with the project it belongs to.

use gpui_kit::*;

use super::threads::{chat_row, place};
use crate::{ui::widgets, workspace::Workspace};

/// How many chats the list shows.
const SHOWN: usize = 30;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let threads: Vec<_> = super::listed(ws, cx)
        .into_iter()
        .take(SHOWN)
        .cloned()
        .collect();
    let mut list = div().flex().flex_col().gap_0p5().pt_1();
    if threads.is_empty() {
        return list.child(widgets::muted("No chats yet.", cx));
    }
    for thread in &threads {
        list = list.child(chat_row(ws, thread, place(ws, thread), cx));
    }
    list
}
