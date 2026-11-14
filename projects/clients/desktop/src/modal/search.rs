//! The chat search: a field over the recent chats, filtered as the user types.

use gpui_kit::{
    component::{ActiveTheme, input::Input},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

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
                .when(thread.source != "gui", |row| {
                    row.child(
                        div()
                            .meta()
                            .text_color(muted.opacity(0.8))
                            .child(thread.source.clone()),
                    )
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = None;
                    ws.open_chat(&id, cx);
                })),
        );
    }
    let query = ws.search.read(cx).value().trim().to_owned();
    let mut said = div().flex().flex_col().gap_0p5().p_2();
    for (index, found) in ws.found.1.iter().enumerate() {
        let (id, words) = (found.thread_id.clone(), ws.found.0.clone());
        said = said.child(
            widgets::row(("said", index), false, cx)
                .h_auto()
                .py_1p5()
                .flex_col()
                .items_start()
                .gap_0p5()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .meta()
                        .text_color(muted)
                        .child(found.title.clone().unwrap_or_else(|| "New chat".to_owned())),
                )
                .child(
                    div()
                        .w_full()
                        .small()
                        .line_clamp(2)
                        .child(found.snippet.clone()),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = None;
                    ws.jump = Some((id.clone(), words.clone()));
                    ws.open_chat(&id, cx);
                })),
        );
    }
    let searched = query.chars().count() >= 3 && ws.found.0 == query;
    div()
        .flex()
        .flex_col()
        .child(div().p_3().child(Input::new(&ws.search)))
        .child(widgets::caption(
            if query.is_empty() { "Recent" } else { "Chats" },
            cx,
        ))
        .child(list)
        .when(!query.is_empty(), |body| {
            body.child(widgets::caption("In messages", cx)).child(
                if !ws.found.1.is_empty() && searched {
                    said
                } else {
                    div().p_2().child(widgets::muted(
                        if query.chars().count() < 3 {
                            "Type three characters to search what was said."
                        } else if searched {
                            "Nothing said matches."
                        } else {
                            "Searching..."
                        },
                        cx,
                    ))
                },
            )
        })
}

impl Workspace {
    /// Asks the core for the messages that contain what is typed. An answer for words
    /// typed over meanwhile is dropped.
    pub fn search_text(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_owned();
        if query.chars().count() < 3 || self.found.0 == query {
            return;
        }
        let asked = query.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().search(&asked).await },
            move |ws, result, cx| {
                if ws.search.read(cx).value().trim() != query {
                    return;
                }
                match result {
                    Ok(found) => ws.found = (query, found),
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
    }
}
