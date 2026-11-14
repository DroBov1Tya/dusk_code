//! The shown chat's task board, as its agents keep it: each task a card with how far it
//! is, and its items down a line, the one being worked on lit.

use std::time::Duration;

use gpui_kit::{
    component::{ActiveTheme, Colorize, IconName},
    prelude::FluentBuilder,
    *,
};

use dusk_protocol::thread::{Task, TaskItem};

use crate::{ui::widgets, workspace::Workspace};

/// The marker of an item, and the line that joins it to the next.
const MARK: f32 = 14.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let tasks = ws
        .shown_chat()
        .map(|chat| chat.tasks.as_slice())
        .unwrap_or_default();
    if tasks.is_empty() {
        return div().child(widgets::muted("No tasks in this chat.", cx));
    }
    let mut column = div().flex().flex_col().gap_2().p_2();
    for task in tasks {
        column = column.child(card(task, cx));
    }
    column
}

fn card(task: &Task, cx: &App) -> Div {
    let theme = cx.theme();
    let (accent, muted, track) = (theme.primary, theme.muted_foreground, theme.border);
    let done = task.items.iter().filter(|item| item.done).count();
    let all = task.items.len();
    let finished = task.done || (all > 0 && done == all);
    let share = if all == 0 {
        f32::from(u8::from(finished))
    } else {
        done as f32 / all as f32
    };
    let fill = if finished { theme.success } else { accent };
    let mut list = div().flex().flex_col();
    for (index, item) in task.items.iter().enumerate() {
        list = list.child(row(task, item, index + 1 == all, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .bg(theme.muted)
        // A task that is over steps back for the ones still open.
        .when(finished, |card| card.opacity(0.72))
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(task.description.clone()),
                )
                .when(all > 0, |head| {
                    head.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(if finished { fill } else { muted })
                            .child(format!("{done}/{all}")),
                    )
                }),
        )
        .child(
            div()
                .h(px(3.))
                .w_full()
                .rounded_full()
                .bg(track)
                .overflow_hidden()
                .child(div().h_full().rounded_full().bg(fill).w(relative(share))),
        )
        .when(all > 0, |card| card.child(list))
}

/// One item: its marker on the line, then its words. Done reads struck and quiet; the
/// one in work is the only thing in the card that moves.
fn row(task: &Task, item: &TaskItem, last: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let (accent, success, muted, line) = (
        theme.primary,
        theme.success,
        theme.muted_foreground,
        theme.border,
    );
    let active = !item.done && item.started_at.is_some();
    let mark = if item.done {
        div()
            .size(px(MARK))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(success.opacity(0.18))
            .child(
                widgets::icon(IconName::Check, success)
                    .size(px(10.))
                    .into_any_element(),
            )
            .into_any_element()
    } else if active {
        div()
            .size(px(MARK))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(accent)
            .child(div().size(px(6.)).rounded_full().bg(accent))
            .with_animation(
                SharedString::from(format!("todo-now-{}-{}", task.task_id, item.item_id)),
                Animation::new(Duration::from_millis(1800))
                    .repeat()
                    .with_easing(ease_in_out),
                |mark, at| mark.opacity(0.55 + 0.45 * (1. - (2. * at - 1.).abs())),
            )
            .into_any_element()
    } else {
        div()
            .size(px(MARK))
            .rounded_full()
            .border_1()
            .border_color(muted.opacity(0.5))
            .into_any_element()
    };
    div()
        .flex()
        .flex_row()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(px(MARK))
                .flex()
                .flex_col()
                .items_center()
                .pt(px(3.))
                .child(mark)
                // The line runs on to the next marker, however tall this item's text is.
                .when(!last, |rail| {
                    rail.child(div().flex_1().min_h(px(6.)).w(px(1.)).my(px(2.)).bg(line))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pb_2()
                .text_xs()
                .when(item.done, |text| text.text_color(muted).line_through())
                .when(active, |text| {
                    text.text_color(theme.foreground.mix_oklab(accent, 0.25))
                })
                .child(item.description.clone()),
        )
}
