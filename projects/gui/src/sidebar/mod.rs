//! The left column: new chat, search, artifacts, connectors and settings on top, then
//! the projects with their chats, and every chat by recent activity at the bottom.

mod projects;
mod recent;
mod threads;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

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
            nav_row("nav-new", IconName::Plus, "New chat", false, cx)
                .on_click(cx.listener(|ws, _, window, cx| ws.new_chat(None, window, cx))),
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
            nav_row(
                "nav-artifacts",
                IconName::LayoutDashboard,
                "Artifacts",
                ws.center == Center::Artifacts,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Artifacts;
                ws.load_outputs(cx);
            })),
        )
        .child(
            nav_row(
                "nav-connectors",
                IconName::Network,
                "Connectors",
                ws.center == Center::Connectors,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Connectors;
                ws.load_settings(cx);
            })),
        )
        .child(
            nav_row(
                "nav-settings",
                IconName::Settings,
                "Settings",
                ws.center == Center::Settings,
                cx,
            )
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
        .child(
            div().flex().flex_row().justify_end().px_2().pt_2().child(
                Button::new("sidebar-hide")
                    .ghost()
                    .small()
                    .icon(IconName::PanelLeftClose)
                    .tooltip("Hide the sidebar")
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.left_hidden = true;
                        cx.notify();
                    })),
            ),
        )
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

/// The narrow column left when the sidebar is hidden: the same ways in, as icons.
pub fn rail(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border, muted) = (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
    let button = |id: &'static str, icon: IconName, tip: &'static str, active: bool, cx: &App| {
        widgets::row(id, active, cx)
            .w(px(36.))
            .justify_center()
            .px_0()
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
            })
            .child(widgets::icon(icon, muted))
    };
    div()
        .w(px(52.))
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .pt_3()
        .bg(panel)
        .border_r_1()
        .border_color(border)
        .child(
            button(
                "rail-open",
                IconName::PanelLeftOpen,
                "Show the sidebar",
                false,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.left_hidden = false;
                cx.notify();
            })),
        )
        .child(
            button("rail-new", IconName::Plus, "New chat", false, cx)
                .on_click(cx.listener(|ws, _, window, cx| ws.new_chat(None, window, cx))),
        )
        .child(
            button("rail-search", IconName::Search, "Search", false, cx).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.modal = Some(Modal::Search);
                    cx.notify();
                },
            )),
        )
        .child(
            button(
                "rail-artifacts",
                IconName::LayoutDashboard,
                "Artifacts",
                ws.center == Center::Artifacts,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Artifacts;
                ws.load_outputs(cx);
            })),
        )
        .child(
            button(
                "rail-connectors",
                IconName::Network,
                "Connectors",
                ws.center == Center::Connectors,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Connectors;
                ws.load_settings(cx);
            })),
        )
        .child(
            button(
                "rail-settings",
                IconName::Settings,
                "Settings",
                ws.center == Center::Settings,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Settings;
                ws.load_settings(cx);
            })),
        )
}

fn nav_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    widgets::row(id, active, cx)
        .h(px(32.))
        .child(widgets::icon(icon, muted))
        .child(label)
}
