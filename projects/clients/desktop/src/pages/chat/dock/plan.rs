//! The plan strip: one 32 px line folded, with the plan's counts, its bar and the task in
//! work; unfolded, the plan's numbered rows, six in sight and the rest behind a scroll that
//! lays out only the rows in sight, so two hundred tasks cost what six do.

use std::ops::Range;

use dusk_protocol::thread::{PlanCounts, Task, TaskStatus};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

const LINE: f32 = 32.;
const ROW: f32 = 28.;
const IN_SIGHT: usize = 6;
/// The bar's widest, whatever the plan's size.
const BAR: f32 = 160.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = ws.shown_chat()?;
    let counts = ws.dock.counts_of(ws.shown.as_deref());
    let all = counts.map_or(0, total);
    if chat.tasks.is_empty() && all == 0 {
        return None;
    }
    let open = ws.dock.plan_open;
    let theme = cx.theme();
    let (muted, border, surface) = (theme.muted_foreground, theme.border, theme.muted);
    let summary = match counts {
        Some(counts) => div()
            .small()
            .text_color(theme.foreground)
            .whitespace_nowrap()
            .child(summary(counts))
            .into_any_element(),
        // The counts are on their way: a still placeholder of their size.
        None => div()
            .w(px(120.))
            .h(px(8.))
            .rounded_full()
            .bg(border)
            .into_any_element(),
    };
    let current = current(&chat.tasks).map(|(number, task)| {
        div()
            .meta()
            .text_color(muted)
            .truncate()
            .child(format!("#{number} {}", one_line(&task.description)))
    });
    let line = div()
        .id("plan-line")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(LINE))
        .px_2()
        .rounded_lg()
        .cursor_pointer()
        .hover(|line| line.bg(cx.theme().list_hover))
        .focusable_row(cx)
        .child(
            widgets::icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                muted,
            )
            .flex_none(),
        )
        .child(
            div()
                .flex_none()
                .group_name()
                .text_color(muted)
                .child("Plan"),
        )
        .child(summary)
        .child(div().flex_1().min_w_0().children(current))
        .children(counts.map(|counts| bar(counts, cx)))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_plan(cx)));
    let rows = open.then(|| {
        let shown = chat.tasks.len().min(IN_SIGHT);
        uniform_list(
            "plan-rows",
            chat.tasks.len(),
            cx.processor(|ws: &mut Workspace, range: Range<usize>, _, cx| {
                let tasks = ws
                    .shown_chat()
                    .map(|chat| chat.tasks.as_slice())
                    .unwrap_or_default();
                range
                    .filter_map(|index| tasks.get(index).map(|task| row(index + 1, task, cx)))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&ws.dock.rows)
        .h(px(shown as f32 * ROW))
        .border_t_1()
        .border_color(border)
    });
    Some(
        div()
            .px_6()
            .pb_2()
            .max_w(px(780.))
            .mx_auto()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(surface)
                    .child(line)
                    .children(rows),
            ),
    )
}

/// "3 of 7 done · 1 running · 1 blocked": the parts that are not zero after the done.
fn summary(counts: PlanCounts) -> String {
    let mut text = format!("{} of {} done", counts.done, total(counts));
    for (count, word) in [
        (counts.running, "running"),
        (counts.claimed, "claimed"),
        (counts.review, "in review"),
        (counts.blocked, "blocked"),
        (counts.failed, "failed"),
    ] {
        if count > 0 {
            text.push_str(&format!(" \u{00B7} {count} {word}"));
        }
    }
    text
}

fn total(counts: PlanCounts) -> u32 {
    let PlanCounts {
        open,
        claimed,
        running,
        blocked,
        review,
        done,
        failed,
    } = counts;
    [open, claimed, running, blocked, review, done, failed]
        .into_iter()
        .fold(0, u32::saturating_add)
}

/// The task the folded line names, with its number: the first running, else the first
/// that is not over.
fn current(tasks: &[Task]) -> Option<(usize, &Task)> {
    let numbered = || {
        tasks
            .iter()
            .enumerate()
            .map(|(index, task)| (index + 1, task))
    };
    numbered()
        .find(|(_, task)| task.status == TaskStatus::Running)
        .or_else(|| {
            numbered()
                .find(|(_, task)| !matches!(task.status, TaskStatus::Done | TaskStatus::Failed))
        })
}

/// The plan as one bar of fixed width: done, running, failed and blocked shares in their
/// colours on the track; it never grows with the number of tasks.
fn bar(counts: PlanCounts, cx: &App) -> Div {
    let theme = cx.theme();
    let all = total(counts).max(1) as f32;
    let mut bar = div()
        .flex_none()
        .flex()
        .flex_row()
        .w(px(BAR))
        .h(px(4.))
        .rounded_full()
        .overflow_hidden()
        .bg(theme.border);
    for (count, color) in [
        (counts.done, theme.success),
        (counts.running, theme.primary),
        (counts.failed, theme.danger),
        (counts.blocked, theme.warning),
    ] {
        if count > 0 {
            bar = bar.child(div().h_full().w(px(BAR * count as f32 / all)).bg(color));
        }
    }
    bar
}

/// One task: its number, the state's glyph and word, its words on one line and its owner.
fn row(number: usize, task: &Task, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let (glyph, color) = look(task.status, cx);
    let owner = task.owner.as_ref().map(|owner| {
        div()
            .flex_none()
            .max_w(px(96.))
            .meta()
            .text_color(muted)
            .truncate()
            .child(owner.name.clone())
    });
    // A list item sizes to its content unless told otherwise; full width keeps the status
    // words in one column at the right edge.
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(ROW))
        .px_2()
        .child(
            div()
                .flex_none()
                .w(px(24.))
                .meta()
                .text_color(muted)
                .text_right()
                .child(number.to_string()),
        )
        .child(
            div()
                .flex_none()
                .w(px(14.))
                .text_center()
                .small()
                .text_color(color)
                .child(glyph),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .small()
                .truncate()
                .when(task.status == TaskStatus::Done, |text| {
                    text.text_color(muted)
                })
                .child(one_line(&task.description)),
        )
        .children(owner)
        .child(
            div()
                .flex_none()
                .w(px(56.))
                .meta()
                .text_color(color)
                .text_right()
                .child(task.status.name()),
        )
}

/// The glyph and colour of a status; its word is always beside it, so colour is never
/// the only sign.
fn look(status: TaskStatus, cx: &App) -> (&'static str, Hsla) {
    let theme = cx.theme();
    match status {
        TaskStatus::Open => ("\u{25CB}", theme.muted_foreground),
        TaskStatus::Claimed => ("\u{25C9}", theme.info),
        TaskStatus::Running => ("\u{25CF}", theme.primary),
        TaskStatus::Blocked => ("\u{25C6}", theme.warning),
        TaskStatus::Review => ("\u{25C7}", theme.info),
        TaskStatus::Done => ("\u{2713}", theme.success),
        TaskStatus::Failed => ("\u{2715}", theme.danger),
    }
}

/// A task's words are model text: one line, drawn as plain text.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
