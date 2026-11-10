//! The chat search: a field over the recent chats, filtered as the user types.

use gpui_kit::{
    component::{ActiveTheme, input::Input},
    *,
};

use crate::{ui::Type, widgets, workspace::Workspace};

/// How many chats the search shows.
const SHOWN: usize = 40;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut list = div().flex().flex_col().gap_0p5().p_2();
    let threads: Vec<_> = ws
        .threads
        .iter()
        .filter(|thread| ws.matches_search(thread, cx))
        .take(SHOWN)
        .cloned()
        .collect();
    if threads.is_empty() {
        list = list.child(widgets::muted("No chats match.", cx));
    }
    for thread in threads {
        let id = thread.id.clone();
        let project = thread
            .project
            .as_deref()
            .and_then(|dir| ws.projects.iter().find(|p| p.dir.to_string_lossy() == dir))
            .map(|p| p.name.clone());
        list = list.child(
            widgets::row(SharedString::from(format!("found-{id}")), false, cx)
                .h(px(40.))
                .child(
                    div().flex_1().min_w_0().truncate().child(
                        thread
                            .title
                            .clone()
                            .unwrap_or_else(|| "New chat".to_owned()),
                    ),
                )
                .children(project.map(|name| div().meta().text_color(muted).child(name)))
                .child(widgets::badge(thread.source.clone(), muted))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = None;
                    ws.open_chat(&id, cx);
                })),
        );
    }
    div()
        .flex()
        .flex_col()
        .child(div().p_3().child(Input::new(&ws.search)))
        .child(widgets::caption("Recent", cx))
        .child(list)
}
