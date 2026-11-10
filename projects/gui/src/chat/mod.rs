//! The middle column: the shown chat's transcript, the cards that wait for the user,
//! the composer and the bar under it.

mod access_menu;
mod approval;
pub mod code;
pub mod composer;
pub mod links;
mod media;
pub mod messages;
mod mode_menu;
mod model_bar;
mod model_menu;
mod navigator;
pub mod queue;
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
    if let Some(text) = ws.fill_composer.take() {
        ws.composer
            .update(cx, |composer, cx| composer.set_value(text, window, cx));
    }
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
                    "Show or hide the side panel",
                    cx,
                )),
        )
        .children(notice)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(body)
                .children(to_bottom(ws, cx)),
        )
        .child(approval::render(ws, cx))
        .child(composer::render(ws, window, cx))
        .child(model_bar::render(ws, cx))
}

/// A round button over the composer while the chat is scrolled up: one click goes
/// to the newest message and follows the chat again.
fn to_bottom(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    const SHOWN_PAST: f32 = 240.;
    let max = ws.list.max_offset_for_scrollbar().y;
    let scrolled = -ws.list.scroll_px_offset_for_scrollbar().y;
    if max - scrolled < px(SHOWN_PAST) {
        return None;
    }
    Some(
        div()
            .absolute()
            .bottom(px(12.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                Button::new("to-bottom")
                    .rounded(gpui_kit::component::button::ButtonRounded::Size(px(999.)))
                    .icon(IconName::ArrowDown)
                    .tooltip("To the newest message")
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.list.scroll_to_end();
                        ws.list.set_follow_mode(FollowMode::Tail);
                        cx.notify();
                    })),
            ),
    )
}

/// Shows or hides a side column.
fn panel_toggle(
    id: &'static str,
    icon: IconName,
    hidden: fn(&mut Workspace) -> &mut bool,
    tip: &'static str,
    cx: &mut Context<Workspace>,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .tooltip(tip)
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
