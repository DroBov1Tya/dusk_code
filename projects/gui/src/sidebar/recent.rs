//! Every chat by its last activity, wherever it was started, marked with its source.

use gpui_kit::*;

use super::threads::chat_row;
use crate::{widgets, workspace::Workspace};

/// How many chats the list shows.
const SHOWN: usize = 30;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let pinned: Vec<_> = crate::theme::prefs(cx)
        .pinned
        .iter()
        .filter_map(|id| ws.threads.iter().find(|thread| thread.id == *id))
        .cloned()
        .collect();
    let mut top = div().flex().flex_col();
    if !pinned.is_empty() {
        top = top.child(widgets::caption("Pinned", cx));
        for thread in &pinned {
            top = top.child(chat_row(ws, thread, Some(&thread.source), cx));
        }
    }
    let open = !ws.collapsed.contains(RECENT);
    let mut list = div().flex().flex_col().pt_2().child(
        widgets::caption(if open { "Recent  ▾" } else { "Recent  ▸" }, cx)
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
        return top.child(list);
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
    top.child(list)
}

/// The key the recent list folds under, beside the projects' IDs.
const RECENT: &str = "recent";
