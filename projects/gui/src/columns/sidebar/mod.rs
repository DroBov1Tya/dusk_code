//! The sidebar: the projects with their chats, and every chat by recent activity under
//! them. The ways into the rest of the app are on the rail beside it.

mod projects;
mod recent;
mod threads;

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

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
