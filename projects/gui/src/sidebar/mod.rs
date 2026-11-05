//! The left column: new chat, search, artifacts, connectors and settings on top, then
//! the projects with their chats, and every chat by recent activity at the bottom.

mod projects;
mod recent;
mod threads;

use gpui_kit::{component::ActiveTheme, component::IconName, *};

use crate::{
    modal::Modal,
    widgets,
    workspace::{Center, Workspace},
};

pub const WIDTH: f32 = 272.;

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    let nav = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .px_2()
        .pt_3()
        .child(
            nav_row("nav-new", IconName::Plus, "New chat", false, cx).on_click(cx.listener(
                |ws, _, window, cx| ws.new_chat(None, window, cx),
            )),
        )
        .child(
            nav_row("nav-search", IconName::Search, "Search", false, cx).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.modal = Some(Modal::Search);
                    cx.notify();
                },
            )),
        )
        .child(
            nav_row("nav-artifacts", IconName::LayoutDashboard, "Artifacts", ws.center == Center::Artifacts, cx)
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.center = Center::Artifacts;
                    ws.load_outputs(cx);
                })),
        )
        .child(
            nav_row("nav-connectors", IconName::Network, "Connectors", ws.center == Center::Connectors, cx)
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.center = Center::Connectors;
                    ws.load_settings(cx);
                })),
        )
        .child(
            nav_row("nav-settings", IconName::Settings, "Settings", ws.center == Center::Settings, cx)
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.center = Center::Settings;
                    ws.load_settings(cx);
                })),
        );
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(panel)
        .border_r_1()
        .border_color(border)
        .child(nav)
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

fn nav_row(id: &'static str, icon: IconName, label: &'static str, active: bool, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    widgets::row(id, active, cx)
        .h(px(32.))
        .child(widgets::icon(icon, muted))
        .child(label)
}
