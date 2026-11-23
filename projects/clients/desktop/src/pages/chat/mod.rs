//! The middle column: the shown chat's transcript with its pins' drawer on top, the cards
//! that wait for the user, the composer and the bar under it.

mod access_menu;
pub mod activity;
mod approval;
pub mod audio;
pub mod code;
mod commands;
pub mod composer;
pub mod dictation;
pub mod dock;
mod fireflies;
pub mod links;
mod marks;
mod media;
pub mod messages;
pub mod mode_menu;
mod model_bar;
mod model_menu;
mod navigator;
mod parts;
pub mod pins;
pub mod pull_link;
pub mod queue;
mod ring;
pub mod timing;
pub mod transcript;
mod welcome;
pub mod working;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{ui::Type, workspace::Workspace};

pub fn render(
    ws: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    composer::show_draft(ws, window, cx);
    composer::show_suggestion(ws, window, cx);
    code::show(ws.shown.as_ref(), cx);
    let theme = cx.theme();
    let danger = theme.danger;
    // The words can be selected, so the banner goes away from its own button, not from
    // a click anywhere on it, which a drag to select would end with.
    let notice = ws.notice.clone().map(|text| {
        div()
            .mx_4()
            .mt_2()
            .pl_3()
            .pr_1()
            .py_1()
            .flex()
            .items_start()
            .gap_2()
            .rounded_md()
            .meta()
            .text_color(danger)
            .bg(danger.opacity(0.1))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py_1()
                    .child(gpui_kit::base::SelectableText::new("notice-text", text)),
            )
            .child(
                Button::new("notice-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Dismiss")
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.notice = None;
                        cx.notify();
                    })),
            )
    });
    let body = if ws.shown.is_some() {
        messages::render(ws, window, cx).into_any_element()
    } else {
        welcome::render(ws, cx).into_any_element()
    };
    div()
        .id("chat-page")
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
        .children(pins::handle(ws, cx))
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(body)
                .children(fireflies::render(ws, cx))
                .children(to_bottom(ws, cx))
                .children(pins::drawer(ws, cx)),
        )
        .children(dock::plan_strip(ws, cx))
        .child(composer::dock_lane(approval::render(ws, window, cx)))
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
