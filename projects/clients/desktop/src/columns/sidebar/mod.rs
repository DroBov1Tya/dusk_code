//! The sidebar: the chats, each of them once, under a New chat that belongs to no project.
//! They stand under their projects or all together by last activity; which, is the
//! reader's choice and is kept with the look of the window. The ways into the rest of the
//! app are on the rail beside it.

mod projects;
mod recent;
mod threads;

use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub const WIDTH: f32 = 272.;

/// How the chats are listed.
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// Under their projects; the chats of no project after them.
    #[default]
    Project,
    /// One list, the chat touched last first.
    Recent,
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let panel = theme.sidebar;
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .pt_2()
        .bg(panel)
        // A chat of its own, outside every project: it works in the workspace folder.
        .child(
            div().px_2().child(
                widgets::row("sidebar-new-chat", false, cx)
                    .h(px(crate::ui::size::ROW))
                    .child(widgets::icon(IconName::Plus, cx.theme().muted_foreground))
                    .child(
                        div()
                            .body()
                            .font_weight(FontWeight::MEDIUM)
                            .child("New chat"),
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("A chat outside the projects")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|ws, _, window, cx| ws.new_chat(None, window, cx)))
                    .child(div().flex_1())
                    // Beside it, the chat that leaves nothing behind.
                    .child(
                        Button::new("sidebar-temporary-chat")
                            .ghost()
                            .xsmall()
                            .icon(Icon::default().path(crate::ui::icons::TEMPORARY))
                            .tooltip("Temporary chat: kept in memory, gone when the app closes")
                            .on_click(cx.listener(|ws, _, window, cx| {
                                cx.stop_propagation();
                                ws.new_temporary_chat(window, cx);
                            })),
                    ),
            ),
        )
        .child(
            div()
                .id("chats")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb_4()
                .child(switch(cx))
                .children(pinned(ws, cx))
                .child(match crate::ui::theme::prefs(cx).chats_by {
                    Order::Project => projects::render(ws, window, cx),
                    Order::Recent => recent::render(ws, cx),
                }),
        )
}

/// The words that choose the order, and beside them the way to add a project.
fn switch(cx: &mut Context<Workspace>) -> Div {
    let order = crate::ui::theme::prefs(cx).chats_by;
    let word = |id: &'static str, label: &'static str, to: Order, cx: &mut Context<Workspace>| {
        crate::ui::controls::text_choice(id, label, order == to, cx)
            .small()
            .on_click(cx.listener(move |_, _, _, cx| {
                crate::ui::theme::change(cx, |prefs| prefs.chats_by = to);
                cx.notify();
            }))
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .pl_3()
        .pr_3()
        .pt_4()
        .pb_1()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(word("chats-by-project", "By project", Order::Project, cx))
                .child(crate::ui::controls::pipe(cx).small())
                .child(word("chats-by-recent", "Recent", Order::Recent, cx)),
        )
        .child(
            Button::new("add-project")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip("Add a project")
                .on_click(cx.listener(|ws, _, window, cx| ws.open_form(None, window, cx))),
        )
}

/// The chats pinned to the top, in the order they were pinned. A pinned chat stands here
/// and nowhere below.
fn pinned(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let pinned: Vec<_> = crate::ui::theme::prefs(cx)
        .pinned
        .iter()
        .filter_map(|id| ws.threads.iter().find(|thread| thread.id == *id))
        .filter(|thread| ws.matches_search(thread, cx))
        .cloned()
        .collect();
    if pinned.is_empty() {
        return None;
    }
    let mut list = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(widgets::caption("Pinned", cx));
    for thread in &pinned {
        list = list.child(threads::chat_row(
            ws,
            thread,
            threads::place(ws, thread),
            cx,
        ));
    }
    Some(list)
}

/// The chats a list below the pinned ones shows: those the search keeps, less the pinned.
fn listed<'a>(ws: &'a Workspace, cx: &App) -> Vec<&'a dusk_protocol::thread::Thread> {
    let pinned = &crate::ui::theme::prefs(cx).pinned;
    ws.threads
        .iter()
        .filter(|thread| !pinned.contains(&thread.id))
        .filter(|thread| ws.matches_search(thread, cx))
        .collect()
}
