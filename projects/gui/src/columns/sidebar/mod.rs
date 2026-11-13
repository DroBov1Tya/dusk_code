//! The sidebar: the projects with their chats, and every chat by recent activity under
//! them, under a New chat that belongs to none. The ways into the rest of the app are
//! on the rail beside it.

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
                            .icon(Icon::default().path(crate::ui::icons::HOURGLASS))
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
                .child(projects::render(ws, window, cx))
                .child(recent::render(ws, cx)),
        )
}
