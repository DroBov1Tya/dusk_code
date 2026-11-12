//! The thin strip at the window's left edge: the places of the app as icons, always in
//! sight, so the sidebar beside it can hold only the projects and their chats.

use gpui_kit::{
    component::{ActiveTheme, IconName, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::Modal,
    ui::widgets::{self, Focusable},
    workspace::{Center, Workspace},
};

pub const WIDTH: f32 = 44.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    let restart = ws.update_ready.clone().map(|tag| {
        let tip = format!("Restart into {tag}");
        item("rail-restart", IconName::RefreshCw, tip, false, cx)
            .text_color(cx.theme().primary)
            .on_click(cx.listener(|_, _, _, cx| cx.restart()))
    });
    let settings_tip = match &ws.status.available_update {
        Some(tag) => format!("Settings: {tag} is out"),
        None => "Settings".to_owned(),
    };
    div()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .py_2()
        .bg(panel)
        .border_r_1()
        .border_color(border)
        .child(
            item(
                "rail-sidebar",
                if ws.left_hidden {
                    IconName::PanelLeftOpen
                } else {
                    IconName::PanelLeftClose
                },
                "Show or hide the chats".into(),
                false,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.left_hidden = !ws.left_hidden;
                cx.notify();
            })),
        )
        .child(
            item("rail-new", IconName::Plus, "New chat".into(), false, cx)
                .on_click(cx.listener(|ws, _, window, cx| ws.new_chat(None, window, cx))),
        )
        .child(
            item("rail-search", IconName::Search, "Search".into(), false, cx).on_click(
                cx.listener(|ws, _, _, cx| {
                    ws.modal = Some(Modal::Search);
                    cx.notify();
                }),
            ),
        )
        .child(div().w(px(20.)).h(px(1.)).my_1().bg(border))
        .child(
            item(
                "rail-chats",
                IconName::Inbox,
                "Chats".into(),
                matches!(ws.center, Center::Chat | Center::Review | Center::File),
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Chat;
                cx.notify();
            })),
        )
        .child(
            item(
                "rail-schedules",
                IconName::Calendar,
                "Scheduled tasks".into(),
                ws.center == Center::Schedules,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.open_schedules(cx))),
        )
        .child(
            item(
                "rail-artifacts",
                IconName::LayoutDashboard,
                "Artifacts".into(),
                ws.center == Center::Artifacts,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Artifacts;
                ws.load_outputs(cx);
            })),
        )
        .child(
            item(
                "rail-connectors",
                IconName::Network,
                "Connectors".into(),
                ws.center == Center::Connectors,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Connectors;
                ws.load_settings(cx);
            })),
        )
        .child(div().flex_1())
        .children(restart)
        .child(
            item(
                "rail-settings",
                IconName::Settings,
                settings_tip,
                ws.center == Center::Settings,
                cx,
            )
            // A dot on the gear says a release is waiting, without words in the strip.
            .when(ws.status.available_update.is_some(), |item| {
                item.relative().child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
            })
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Settings;
                ws.load_settings(cx);
            })),
        )
}

/// One place: a 32 px square with its icon, filled while it is the shown place.
fn item(id: &'static str, icon: IconName, tip: String, active: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, selected) = (theme.list_hover, theme.list_active);
    let color = if active {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let tip = SharedString::from(tip);
    div()
        .id(id)
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .cursor_pointer()
        .when(active, |item| item.bg(selected))
        .hover(move |item| item.bg(hover))
        .focusable_row(cx)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(widgets::icon(icon, color))
}
