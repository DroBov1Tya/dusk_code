//! The middle column: the shown chat's transcript, the cards that wait for the user,
//! the composer and the bar under it.

mod approval;
mod composer;
pub mod links;
mod media;
pub mod messages;
mod mode_menu;
mod model_bar;
mod model_menu;
mod navigator;
mod ring;
pub mod transcript;
mod working;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, danger) = (theme.border, theme.danger);
    let title = ws
        .shown_chat()
        .and_then(|chat| chat.title.clone())
        .or_else(|| {
            let shown = ws.shown.as_deref()?;
            ws.threads.iter().find(|t| t.id == shown)?.title.clone()
        })
        .unwrap_or_else(|| "New chat".to_owned());
    let project = ws
        .selected_project()
        .map(|project| project.name.clone())
        .unwrap_or_default();
    let notice = ws.notice.clone().map(|text| {
        div()
            .id("notice")
            .mx_4()
            .mt_2()
            .px_3()
            .py_2()
            .rounded_md()
            .text_xs()
            .text_color(danger)
            .bg(danger.opacity(0.1))
            .cursor_pointer()
            .child(text)
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.notice = None;
                cx.notify();
            }))
    });
    let body = if ws.shown.is_some() {
        messages::render(ws, window, cx).into_any_element()
    } else {
        empty(ws, cx).into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(44.))
                .px_4()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(title),
                )
                .when(!project.is_empty(), |header| {
                    header.child(widgets::badge(project, cx.theme().muted_foreground))
                })
                .child(div().flex_1())
                .child(panel_toggle(
                    "toggle-right",
                    IconName::PanelRight,
                    |ws| &mut ws.right_hidden,
                    cx,
                )),
        )
        .children(notice)
        .child(div().flex_1().min_h_0().child(body))
        .child(approval::render(ws, cx))
        .child(composer::render(ws, window, cx))
        .child(model_bar::render(ws, cx))
}

/// Shows or hides a side column.
fn panel_toggle(
    id: &'static str,
    icon: IconName,
    hidden: fn(&mut Workspace) -> &mut bool,
    cx: &mut Context<Workspace>,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .on_click(cx.listener(move |ws, _, _, cx| {
            let hidden = hidden(ws);
            *hidden = !*hidden;
            cx.notify();
        }))
}

fn empty(ws: &Workspace, cx: &App) -> Div {
    let text = if ws.projects.is_empty() {
        "Add a project on the left to start."
    } else {
        "Pick a chat on the left, or start one with + next to a project."
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
