//! The left column: chats, connectors and settings on top, then search, the projects with
//! their chats, and every chat by recent activity at the bottom.

mod projects;
mod recent;
mod threads;

use gpui_kit::{
    component::{ActiveTheme, IconName, input::Input},
    *,
};

use crate::{
    widgets,
    workspace::{Center, Workspace},
};

pub const WIDTH: f32 = 280.;

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border, muted) = (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
    let body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(div().px_2().pb_2().child(Input::new(&ws.search)))
        .child(
            div()
                .id("chats")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(projects::render(ws, window, cx))
                .child(recent::render(ws, cx)),
        );
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(panel)
        .border_r_1()
        .border_color(border)
        .child(
            div()
                .flex()
                .flex_col()
                .py_2()
                .child(nav(ws, "nav-chats", IconName::Inbox, "Chats", ws.center == Center::Chat, muted, cx)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.center = Center::Chat;
                        cx.notify();
                    })))
                .child(nav(ws, "nav-connectors", IconName::Network, "Connectors", ws.center == Center::Connectors, muted, cx)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.center = Center::Connectors;
                        ws.load_settings(cx);
                    })))
                .child(nav(ws, "nav-settings", IconName::Settings, "Settings", ws.center == Center::Settings, muted, cx)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.center = Center::Settings;
                        ws.load_settings(cx);
                    }))),
        )
        .child(body)
}

fn nav(
    _ws: &Workspace,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    active: bool,
    muted: Hsla,
    cx: &App,
) -> Stateful<Div> {
    widgets::row(id, active, cx)
        .child(widgets::icon(icon, muted))
        .child(label)
}
