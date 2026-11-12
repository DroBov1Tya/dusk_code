//! The bar across the top of the work area, out to the window's right edge: the way back
//! and forward and where you are on the left, the tools of that place on the right. The
//! side panel starts under it.

pub mod history;

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName,
        button::{Button, ButtonVariants},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::Modal,
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

pub const HEIGHT: f32 = 52.;

pub fn render(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
    let chat = ws.center == Center::Chat && ws.shown.is_some();
    let menu_x = window.viewport_size().width - px(220.);
    div()
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .pl_2()
        .pr_3()
        .border_b_1()
        .border_color(border)
        .child(
            tool(
                "top-rail",
                IconName::PanelLeft,
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
            tool("top-back", IconName::ArrowLeft, "Back")
                .disabled(!ws.history.can_go_back())
                .on_click(cx.listener(|ws, _, _, cx| ws.go(true, cx))),
        )
        .child(
            tool("top-forward", IconName::ArrowRight, "Forward")
                .disabled(!ws.history.can_go_forward())
                .on_click(cx.listener(|ws, _, _, cx| ws.go(false, cx))),
        )
        .child(div().w(px(1.)).h(px(20.)).mx_2().bg(border))
        .child(title(ws, muted))
        .child(div().flex_1())
        .when(chat, |bar| {
            bar.child(
                tool("top-review", IconName::Inspector, "Review the changes")
                    .on_click(cx.listener(|ws, _, _, cx| ws.open_review(cx))),
            )
        })
        .child(
            tool("top-search", IconName::Search, "Search the chats").on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.modal = Some(Modal::Search);
                    cx.notify();
                },
            )),
        )
        .when_some(ws.shown.clone().filter(|_| chat), |bar, thread_id| {
            // The same menu a right click on the chat in the sidebar opens.
            bar.child(
                tool("top-more", IconName::Ellipsis, "More").on_click(cx.listener(
                    move |ws, _, _, cx| {
                        ws.chat_menu = Some((thread_id.clone(), point(menu_x, px(HEIGHT))));
                        cx.notify();
                    },
                )),
            )
        })
        .when(ws.side_panel_allowed(), |bar| {
            bar.child(
                tool(
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
                })),
            )
        })
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
}

/// A tool of the bar: an icon in a 32 px square.
fn tool(id: &'static str, icon: IconName, tip: &'static str) -> Button {
    Button::new(id).ghost().icon(icon).tooltip(tip)
}
