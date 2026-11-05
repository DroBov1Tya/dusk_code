//! Every chat by its last activity, wherever it was started, marked with its source.

use gpui_kit::*;

use super::threads::chat_row;
use crate::{widgets, workspace::Workspace};

/// How many chats the list shows.
const SHOWN: usize = 30;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let open = !ws.collapsed.contains(RECENT);
    let mut list = div().flex().flex_col().pt_2().child(
        widgets::heading(if open { "Recent  ▾" } else { "Recent  ▸" }, cx)
            .id("recent-toggle")
            .cursor_pointer()
            .on_click(cx.listener(|ws, _, _, cx| {
                if !ws.collapsed.remove(RECENT) {
                    ws.collapsed.insert(RECENT.to_owned());
                }
                cx.notify();
            })),
    );
    if !open {
        return list;
    }
    let threads: Vec<_> = ws
        .threads
        .iter()
        .filter(|thread| ws.matches_search(thread, cx))
        .take(SHOWN)
        .cloned()
        .collect();
    for thread in &threads {
        list = list.child(chat_row(ws, thread, Some(&thread.source), cx));
    }
    list
}

/// The key the recent list folds under, beside the projects' IDs.
const RECENT: &str = "recent";
