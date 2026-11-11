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
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::Modal,
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

pub const WIDTH: f32 = 272.;

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    let restart = ws.update_ready.clone().map(|tag| {
        nav_row(
            "nav-restart",
            IconName::RefreshCw,
            "Restart to update",
            false,
            cx,
        )
        .text_color(cx.theme().primary)
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(format!("{tag} is ready")).build(window, cx)
        })
        .on_click(cx.listener(|_, _, _, cx| cx.restart()))
    });
    let nav = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .px_2()
        .pt_3()
        .children(restart)
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
            .when_some(ws.status.available_update.clone(), |row, _| {
                row.child(widgets::badge("new release!", cx.theme().primary))
            })
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
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pl_4()
                .pr_2()
                .pt_2()
                .child(wordmark(cx))
                .child(
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

/// The name at the top: "Dusk" from violet to a sunset orange, "code" plain.
fn wordmark(cx: &App) -> Div {
    // Violet, magenta, rose, orange: one hue per letter along the dusk sky.
    const DUSK: [(f32, f32, f32); 4] = [
        (268., 0.72, 0.66),
        (300., 0.62, 0.64),
        (338., 0.74, 0.64),
        (24., 0.92, 0.60),
    ];
    let mut word = div()
        .flex()
        .flex_row()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD);
    for (letter, (hue, saturation, lightness)) in "Dusk".chars().zip(DUSK) {
        word = word.child(
            div()
                .text_color(hsla(hue / 360., saturation, lightness, 1.))
                .child(letter.to_string()),
        );
    }
    word.child(div().text_color(cx.theme().foreground).child("code"))
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
        .h(px(crate::ui::NAV))
        .body()
        .font_weight(FontWeight::MEDIUM)
        .child(widgets::icon(icon, muted))
        .child(label)
}
