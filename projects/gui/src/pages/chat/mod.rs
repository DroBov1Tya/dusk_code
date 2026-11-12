//! The middle column: the shown chat's transcript, the cards that wait for the user,
//! the composer and the bar under it.

mod access_menu;
mod approval;
pub mod audio;
pub mod code;
mod commands;
pub mod composer;
pub mod dictation;
mod fireflies;
pub mod links;
mod media;
pub mod messages;
pub mod mode_menu;
mod model_bar;
mod model_menu;
mod navigator;
mod parts;
pub mod queue;
mod ring;
pub mod timing;
pub mod transcript;
mod welcome;
mod working;

use gpui_kit::{
    component::{ActiveTheme, IconName, button::Button},
    *,
};

use crate::{ui::Type, workspace::Workspace};

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    if let Some(text) = ws.fill_composer.take() {
        ws.composer
            .update(cx, |composer, cx| composer.set_value(text, window, cx));
    }
    let theme = cx.theme();
    let danger = theme.danger;
    let notice = ws.notice.clone().map(|text| {
        div()
            .id("notice")
            .mx_4()
            .mt_2()
            .px_3()
            .py_2()
            .rounded_md()
            .meta()
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
        welcome::render(ws, cx).into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        // Files and pictures dragged in from outside go with the next message.
        .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().primary.opacity(0.06)))
        .on_drop(cx.listener(|ws, paths: &ExternalPaths, _, cx| {
            ws.attachments.extend(paths.paths().iter().cloned());
            cx.notify();
        }))
        .children(notice)
        .children(crate::pages::schedules::report::render(ws, cx))
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(body)
                .children(fireflies::render(ws, cx))
                .children(to_bottom(ws, cx)),
        )
        .child(approval::render(ws, cx))
        .child(composer::render(ws, window, cx))
        .child(div().h(px(12.)))
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
