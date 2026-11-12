//! The bar across the top of the work area, out to the window's right edge: where you are
//! on the left, the tools of that place on the right. The side panel starts under it.

use gpui_kit::{
    component::{
        ActiveTheme, IconName,
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

pub const HEIGHT: f32 = 52.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
    let (title, project) = title(ws);
    div()
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pl_5()
        .pr_3()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .min_w_0()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(title),
        )
        .when_some(project, |bar, project| {
            bar.child(widgets::badge(project, muted))
        })
        .child(div().flex_1())
        .when(ws.center == Center::Chat && ws.shown.is_some(), |bar| {
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

/// What the bar names: the chat and its project, or the page.
fn title(ws: &Workspace) -> (String, Option<String>) {
    let page = match ws.center {
        Center::Chat => None,
        Center::Artifacts => Some("Artifacts"),
        Center::Connectors => Some("Connectors"),
        Center::Settings => Some("Settings"),
        Center::Schedules => Some("Scheduled tasks"),
        Center::Review => Some("Changes"),
        Center::File => Some("File"),
    };
    if let Some(page) = page {
        return (page.to_owned(), None);
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
    (chat, project)
}

/// A tool of the bar: an icon in a 32 px square.
fn tool(id: &'static str, icon: IconName, tip: &'static str) -> Button {
    Button::new(id).ghost().icon(icon).tooltip(tip)
}
