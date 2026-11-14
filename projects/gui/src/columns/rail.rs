//! The thin strip at the window's left edge: the places of the app as icons, always in
//! sight, so the sidebar beside it can hold only the projects and their chats.

use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use crate::{
    columns::flyout::Flyout,
    modal::Modal,
    ui::layout::Part,
    ui::widgets::Focusable,
    workspace::{Center, Workspace},
};

pub const WIDTH: f32 = crate::ui::size::BAR;

/// The frame's color, which the rail and the header stand on: a step darker than the
/// sidebar, so the edge between them reads without a line.
pub fn color(cx: &App) -> Hsla {
    let theme = cx.theme();
    let mut color = theme.sidebar;
    color.l *= if theme.mode.is_dark() { 0.86 } else { 0.95 };
    color
}

/// The rail's width as drawn: none while the user keeps it hidden.
pub fn width(ws: &Workspace) -> f32 {
    if ws.rail_hidden { 0. } else { WIDTH }
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let parts = crate::ui::theme::prefs(cx).layout.rail.clone();
    let mut restart = ws.update_ready.clone().map(|tag| {
        let tip = format!("Restart into {tag}");
        item("rail-restart", IconName::RefreshCw, tip, false, cx)
            .text_color(cx.theme().primary)
            .on_click(cx.listener(|_, _, _, cx| cx.restart()))
    });
    let mut rail = div()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_1p5()
        .py_2()
        .bg(color(cx));
    for part in parts {
        // The restart into a waiting update stands just above the far end's group.
        if part == Part::Spacer {
            rail = rail.child(div().flex_1()).children(restart.take());
            continue;
        }
        rail = rail.children(place(ws, part, cx));
    }
    // With no spacer in the map the restart still shows, at the end.
    rail.children(restart)
}

/// The element of one part of the map, or none for a part the rail does not hold.
fn place(ws: &Workspace, part: Part, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let border = cx.theme().sidebar_border;
    Some(match part {
        Part::Divider => div()
            .w(px(20.))
            .h(px(1.))
            .my_1()
            .bg(border)
            .into_any_element(),
        Part::NewChat => item("rail-new", IconName::Plus, "New chat".into(), false, cx)
            .on_click(cx.listener(|ws, _, window, cx| ws.new_chat(None, window, cx)))
            .into_any_element(),
        Part::Search => item("rail-search", IconName::Search, "Search".into(), false, cx)
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.modal = Some(Modal::Search);
                cx.notify();
            }))
            .into_any_element(),
        Part::Chats => item(
            "rail-chats",
            IconName::Inbox,
            "Chats".into(),
            matches!(ws.center, Center::Chat | Center::Review | Center::File),
            cx,
        )
        .map(|item| with_flyout(item, Flyout::Chats, ws.left_hidden, cx))
        .on_click(cx.listener(|ws, _, _, cx| {
            // A click keeps what the hover showed: the sidebar opens for good.
            ws.flyout = None;
            ws.left_hidden = false;
            ws.center = Center::Chat;
            cx.notify();
        }))
        .into_any_element(),
        Part::Schedules => item(
            "rail-schedules",
            IconName::Calendar,
            "Scheduled tasks".into(),
            ws.center == Center::Schedules,
            cx,
        )
        // A preview only stands in for a sidebar that is not there.
        .map(|item| {
            let shown = ws.center == Center::Schedules;
            with_flyout(item, Flyout::Schedules, ws.left_hidden && !shown, cx)
        })
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.flyout = None;
            ws.open_schedules(cx);
        }))
        .into_any_element(),
        Part::Artifacts => item(
            "rail-artifacts",
            IconName::LayoutDashboard,
            "Artifacts".into(),
            ws.center == Center::Artifacts,
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.center = Center::Artifacts;
            ws.load_outputs(cx);
        }))
        .into_any_element(),
        Part::Connectors => item(
            "rail-connectors",
            IconName::Network,
            "Connectors".into(),
            ws.center == Center::Connectors,
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.center = Center::Connectors;
            ws.load_settings(cx);
        }))
        .into_any_element(),
        Part::Terminal => item(
            "rail-terminal",
            IconName::SquareTerminal,
            if ws.terminal.open {
                "Hide the terminal"
            } else {
                "Terminal"
            }
            .into(),
            ws.terminal.open,
            cx,
        )
        .on_click(cx.listener(|ws, _, window, cx| ws.toggle_terminal(window, cx)))
        .into_any_element(),
        Part::Temporary => item(
            "rail-temporary",
            Icon::default().path(crate::ui::icons::TEMPORARY),
            "Temporary chat: kept in memory, gone when the app closes".into(),
            ws.center == Center::Chat && ws.shown.is_none() && ws.temporary_next,
            cx,
        )
        .on_click(cx.listener(|ws, _, window, cx| ws.new_temporary_chat(window, cx)))
        .into_any_element(),
        Part::Quiet => item(
            "rail-quiet",
            if ws.quiet {
                Icon::default().path(crate::ui::icons::MOON_FILL)
            } else {
                Icon::default().path(crate::ui::icons::MOON)
            },
            if ws.quiet {
                "Leave quiet: show the columns again"
            } else {
                "Quiet: only the chat"
            }
            .into(),
            ws.quiet,
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.quiet = !ws.quiet;
            ws.flyout = None;
            cx.notify();
        }))
        .into_any_element(),
        Part::Settings => {
            let tip = match &ws.status.available_update {
                Some(tag) => format!("Settings: {tag} is out"),
                None => "Settings".to_owned(),
            };
            item(
                "rail-settings",
                IconName::Settings,
                tip,
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
            }))
            .into_any_element()
        }
        _ => return None,
    })
}

/// One place: a 32 px square with its icon, filled while it is the shown place.
fn item(
    id: &'static str,
    icon: impl Into<Icon>,
    tip: String,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
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
        .size(px(crate::ui::size::CONTROL + 8.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .cursor_pointer()
        .when(active, |item| item.bg(selected))
        .hover(move |item| item.bg(hover))
        .focusable_row(cx)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(
            icon.into()
                .size(px(crate::ui::size::ICON_LARGE))
                .text_color(color),
        )
}

/// Shows the place's panel while the pointer rests on its icon, where `allowed`.
fn with_flyout(
    item: Stateful<Div>,
    kind: Flyout,
    allowed: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    if !allowed {
        return item;
    }
    item.on_hover(
        cx.listener(move |ws, inside: &bool, _, cx| ws.hover_flyout(Some(kind), *inside, cx)),
    )
}
