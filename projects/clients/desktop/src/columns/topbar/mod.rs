//! The header across the whole window. Over the rail and the sidebar it holds the way
//! back and forward and the switches of those two columns; past a thin rule, where you
//! are and the tools of that place.

pub mod history;
mod sky;

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Selectable,
        button::{Button, ButtonVariants},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::Modal,
    ui::layout::Part,
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

pub const HEIGHT: f32 = crate::ui::size::BAR;
/// The header in quiet mode.
const QUIET_HEIGHT: f32 = crate::ui::size::CONTROL;
/// The narrowest the navigation part gets, with both side columns hidden.
const NAV_MIN: f32 = 156.;

pub fn render(
    ws: &Workspace,
    nav_width: Pixels,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
    let chat = ws.center == Center::Chat && ws.shown.is_some();
    let menu_x = window.viewport_size().width - px(220.);
    let glow = sky::glow(ws, cx);
    let stars = sky::stars(ws, cx);
    let star = sky::star(ws, cx);
    // Quiet thins the header to a line that only names the chat.
    let height = if ws.quiet { QUIET_HEIGHT } else { HEIGHT };
    div()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .relative()
        .when_some(glow, |bar, glow| bar.bg(glow))
        .children(stars)
        .child(nav(ws, nav_width, border, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .pl_4()
                .pr_3()
                .child(title(ws, muted))
                .children(star)
                .child(div().flex_1())
                .children(tools(ws, chat, menu_x, cx)),
        )
}

/// The header's tools, as the layout map orders them.
fn tools(
    ws: &Workspace,
    chat: bool,
    menu_x: Pixels,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    let parts = crate::ui::theme::prefs(cx).layout.header.clone();
    parts
        .into_iter()
        .filter_map(|part| tool_of(ws, part, chat, menu_x, cx))
        .collect()
}

fn tool_of(
    ws: &Workspace,
    part: Part,
    chat: bool,
    menu_x: Pixels,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    Some(match part {
        Part::Review if chat => tool("top-review", IconName::Github, "Review the changes")
            .on_click(cx.listener(|ws, _, _, cx| ws.open_review(cx)))
            .into_any_element(),
        Part::Search => tool("top-search", IconName::Search, "Search the chats")
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.modal = Some(Modal::Search);
                cx.notify();
            }))
            .into_any_element(),
        // The same menu a right click on the chat in the sidebar opens.
        Part::More if chat => {
            let thread_id = ws.shown.clone()?;
            tool("top-more", IconName::Ellipsis, "More")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.chat_menu = Some((thread_id.clone(), point(menu_x, px(HEIGHT))));
                    cx.notify();
                }))
                .into_any_element()
        }
        Part::Simulator if cfg!(target_os = "macos") => tool(
            "top-simulator",
            Icon::default().path(crate::ui::icons::PHONE),
            if ws.simulator.open {
                "Close the iOS Simulator"
            } else {
                "Open the iOS Simulator"
            },
        )
        .when(ws.simulator.open, |button| button.selected(true))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_simulator(cx)))
        .into_any_element(),
        Part::Plugins => crate::plugins::place::header(ws, cx)?,
        Part::SidePanel if ws.side_panel_allowed() => tool(
            "top-panel",
            if ws.right_hidden {
                IconName::PanelRightOpen
            } else {
                IconName::PanelRightClose
            },
            "Show or hide the side panel",
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.right_hidden = !ws.right_hidden;
            cx.notify();
        }))
        .into_any_element(),
        _ => return None,
    })
}

/// Over the rail and the sidebar: their switches, then Back and Forward. It is as wide as
/// those two columns, so its rule lines up with the sidebar's edge.
fn nav(ws: &Workspace, width: Pixels, border: Hsla, cx: &mut Context<Workspace>) -> Div {
    div()
        .w(width.max(px(NAV_MIN)))
        .flex_none()
        .h(px(28.))
        .flex()
        .flex_row()
        .items_center()
        .gap_0p5()
        .pl_2()
        .border_r_1()
        .border_color(border)
        .child(
            tool(
                "top-rail",
                IconName::Menu,
                if ws.rail_hidden {
                    "Show the places"
                } else {
                    "Hide the places"
                },
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.rail_hidden = !ws.rail_hidden;
                cx.notify();
            })),
        )
        .child(
            tool(
                "top-sidebar",
                if ws.left_hidden {
                    IconName::PanelLeftOpen
                } else {
                    IconName::PanelLeftClose
                },
                "Show or hide the chats",
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.left_hidden = !ws.left_hidden;
                cx.notify();
            })),
        )
        .child(
            tool("top-back", IconName::ArrowLeft, "Back")
                .disabled(!ws.history.can_go_back())
                .on_click(cx.listener(|ws, _, _, cx| ws.go(true, cx))),
        )
        .child(
            tool("top-forward", IconName::ArrowRight, "Forward")
                .disabled(!ws.history.can_go_forward())
                .on_click(cx.listener(|ws, _, _, cx| ws.go(false, cx))),
        )
        // The name fills the room left over the sidebar; with the sidebar hidden there
        // is none, and it steps aside.
        .when(width >= px(NAV_MIN + 110.), |nav| {
            nav.child(div().flex_1())
                .child(wordmark(cx))
                .child(div().w_4())
        })
}

/// The mark and the name: a sun half under the horizon with the evening star over it,
/// then "Dusk" from violet to a sunset orange, one hue per letter, and "code" quiet
/// beside it.
fn wordmark(cx: &App) -> Div {
    const DUSK: [(f32, f32, f32); 4] = [
        (268., 0.72, 0.68),
        (300., 0.62, 0.66),
        (338., 0.76, 0.66),
        (24., 0.92, 0.62),
    ];
    let mut word = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(emblem());
    let mut name = div()
        .flex()
        .flex_row()
        .text_size(rems(1.02))
        .font_weight(FontWeight::BOLD);
    for (letter, (hue, saturation, lightness)) in "Dusk".chars().zip(DUSK) {
        name = name.child(
            div()
                .text_color(hsla(hue / 360., saturation, lightness, 1.))
                .child(letter.to_string()),
        );
    }
    word = word.child(
        name.child(
            div()
                .font_weight(FontWeight::NORMAL)
                .text_color(cx.theme().foreground.opacity(0.72))
                .child("code"),
        ),
    );
    word
}

/// The setting sun: the upper half of a disc in the wordmark's own colors over a thin
/// horizon, and a star up to its right.
fn emblem() -> Div {
    const DISC: f32 = 14.;
    let violet = hsla(268. / 360., 0.72, 0.68, 1.);
    let orange = hsla(24. / 360., 0.92, 0.62, 1.);
    let rose = hsla(338. / 360., 0.76, 0.66, 1.);
    div()
        .relative()
        .flex_none()
        .w(px(20.))
        .h(px(16.))
        // Only the half above the horizon shows.
        .child(
            div()
                .absolute()
                .left(px(3.))
                .top(px(4.))
                .w(px(DISC))
                .h(px(DISC / 2.))
                .overflow_hidden()
                .child(div().size(px(DISC)).rounded_full().bg(linear_gradient(
                    180.,
                    linear_color_stop(orange, 0.),
                    linear_color_stop(rose, 0.55),
                ))),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(12.))
                .w(px(20.))
                .h(px(1.5))
                .rounded_full()
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(violet, 0.),
                    linear_color_stop(orange, 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .left(px(17.))
                .top(px(1.))
                .size(px(2.))
                .rounded_full()
                .bg(hsla(0.12, 0.6, 0.92, 1.)),
        )
}

/// Where you are: the page, or the chat under a folder icon that names its project on
/// hover.
fn title(ws: &Workspace, muted: Hsla) -> Stateful<Div> {
    let page = match ws.center {
        Center::Chat => None,
        Center::Artifacts => Some("Artifacts"),
        Center::Connectors => Some("Connectors"),
        Center::Settings => Some("Settings"),
        Center::Schedules => Some("Scheduled tasks"),
        Center::Review => Some("Changes"),
        Center::File => Some("File"),
    };
    let base = div()
        .id("top-title")
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .body()
        .font_weight(FontWeight::SEMIBOLD);
    if let Some(page) = page {
        return base.child(div().truncate().child(page));
    }
    let chat = ws
        .shown_chat()
        .and_then(|chat| chat.title.clone())
        .or_else(|| {
            let shown = ws.shown.as_ref()?;
            ws.threads.iter().find(|t| &t.id == shown)?.title.clone()
        })
        .unwrap_or_else(|| "New chat".to_owned());
    let project = ws.selected_project().map(|project| project.name.clone());
    base.when_some(project, |title, project| {
        let project = SharedString::from(project);
        title
            .child(widgets::icon(IconName::Folder, muted))
            .tooltip(move |window, cx| Tooltip::new(project.clone()).build(window, cx))
    })
    .child(div().truncate().child(chat))
    // Said beside the title for as long as the chat is shown: nothing of it is kept.
    .when(ws.temporary_shown(), |title| {
        title.child(widgets::badge("temporary", muted))
    })
}

/// A tool of the bar: an icon in a 32 px square.
fn tool(id: &'static str, icon: impl Into<Icon>, tip: &'static str) -> Button {
    Button::new(id).ghost().icon(icon.into()).tooltip(tip)
}
