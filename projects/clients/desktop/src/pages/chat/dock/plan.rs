//! The plan strip: one 32 px line folded, with the plan's counts, its bar and the task in
//! work; unfolded, the plan's numbered rows, six in sight and the rest behind a scroll that
//! lays out only the rows in sight, so two hundred tasks cost what six do.

use crate::ui::elevation::{Elevation, Level};
use crate::ui::press::Press;
use std::ops::Range;

use dusk_protocol::thread::{GoalCheck, PlanCounts, Task, TaskStatus, TeamEnd, task_in_work};
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
/// How long a task that just came takes to draw in.
pub const ARRIVE: std::time::Duration = std::time::Duration::from_millis(220);
/// How long the bar takes to ease to new shares.
const BAR_EASE: std::time::Duration = std::time::Duration::from_millis(300);

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = ws.shown_chat()?;
    let counts = ws.dock.counts_of(ws.shown.as_deref());
    let all = counts.map_or(0, |counts| counts.total());
    if chat.tasks.is_empty() && all == 0 {
        return None;
    }
    let open = ws.dock.plan_open;
    // While the plan is written, its label carries the light.
    let planning = chat
        .work(dusk_protocol::system::now_ms())
        .is_some_and(|work| work.kind == dusk_protocol::thread::WorkKind::Planning);
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let summary = match counts {
        Some(counts) => div()
            .small()
            .text_color(theme.foreground)
            .min_w_0()
            .truncate()
            .child(counts.summary())
            .into_any_element(),
        // The counts are on their way: a still placeholder of their size.
        None => div()
            .w(px(120.))
            .h(px(8.))
            .rounded_full()
            .bg(border)
            .into_any_element(),
    };
    // An ended team says how it ended in the place of a task in work: none is.
    let ended_team = chat
        .team
        .as_ref()
        .is_some_and(|team| team.ended().is_some());
    let ended = chat.team.as_ref().and_then(|team| {
        let end = team.ended()?;
        let color = match end {
            TeamEnd::Done => theme.success,
            _ => theme.warning,
        };
        Some(
            div()
                .meta()
                .text_color(color)
                .truncate()
                .child(team.progress_note()),
        )
    });
    let current = ended.or_else(|| {
        task_in_work(&chat.tasks, chat.team.as_ref()).map(|(number, task)| {
            div()
                .meta()
                .text_color(muted)
                .truncate()
                .child(task.numbered(number))
        })
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
        .press(cx.theme().list_hover, cx)
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
                .child(crate::pages::chat::activity::plan_label(planning, cx)),
        )
        .child(summary)
        // How a team ended keeps its words; the counts give way to them.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(ended_team, |line| line.min_w(px(260.)))
                .children(current),
        )
        .children(counts.map(|counts| bar(counts, ws.dock.counts_before, cx)))
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
                    .filter_map(|index| {
                        let task = tasks.get(index)?;
                        let row = row(index + 1, task, cx);
                        // A task that just came fades in and rises its last 4 px, once.
                        let came = ws.dock.tasks_came.1.get(&task.task_id).copied();
                        let arriving = came.and_then(|came| {
                            crate::pages::chat::activity::arriving(came, ARRIVE, cx).or_else(
                                // Its turn has not come yet: it waits unseen.
                                || (came > std::time::Instant::now()).then_some(0.),
                            )
                        });
                        Some(match arriving {
                            Some(t) => row
                                .opacity(t)
                                .relative()
                                .top(px((1. - t) * 4.))
                                .child(crate::pages::chat::activity::next_frame()),
                            None => row,
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&ws.dock.rows)
        .h(px(shown as f32 * ROW))
        .my_1()
    });
    Some(
        crate::pages::chat::composer::dock_lane(
            div()
                .flex()
                .flex_col()
                .rounded_xl()
                .elevation(Level::Raised, cx)
                .overflow_hidden()
                .child(line)
                // The hairline under the line is the rows' own frame, so their air
                // above and below stays inside the card.
                .children(rows.map(|rows| div().border_t_1().border_color(border).child(rows))),
        )
        .pb_2(),
    )
}

/// The plan as one bar of fixed width: done, running, failed and blocked shares in their
/// colours on the track; it never grows with the number of tasks.
fn bar(counts: PlanCounts, before: Option<(PlanCounts, std::time::Instant)>, cx: &App) -> Div {
    let theme = cx.theme();
    let share = |count: u32, of: PlanCounts| count as f32 / of.total().max(1) as f32;
    // While the shares ease from what they were, each is between the old and the new.
    let easing = before.and_then(|(before, since)| {
        crate::pages::chat::activity::arriving(since, BAR_EASE, cx).map(|t| (before, t))
    });
    let mut bar = div()
        .flex_none()
        .flex()
        .flex_row()
        .w(px(BAR))
        .h(px(4.))
        .rounded_full()
        .overflow_hidden()
        .bg(theme.border);
    type Share = (fn(PlanCounts) -> u32, Hsla);
    let pick: [Share; 4] = [
        (|counts| counts.done, theme.success),
        (|counts| counts.running, theme.primary),
        (|counts| counts.failed, theme.danger),
        (|counts| counts.blocked, theme.warning),
    ];
    for (of, color) in pick {
        let now = share(of(counts), counts);
        let shown = match easing {
            Some((before, t)) => {
                let was = share(of(before), before);
                was + (now - was) * t
            }
            None => now,
        };
        if shown > 0. {
            bar = bar.child(div().h_full().w(px(BAR * shown)).bg(color));
        }
    }
    if easing.is_some() {
        bar = bar.child(crate::pages::chat::activity::next_frame());
    }
    bar
}

/// One task: its number, the state's glyph and word, its words on one line and its owner.
/// A task that waits for the user's confirmation has `confirm` for its word, and a
/// click on it passes the task.
fn row(number: usize, task: &Task, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.primary);
    let (glyph, color) = (task.status.glyph(), widgets::tone(task.status.tone(), cx));
    let word = div().flex_none().w(px(64.)).meta().text_right();
    let for_user =
        task.status == TaskStatus::Review && task.gate == Some(GoalCheck::UserConfirmation);
    let word = match for_user {
        true => {
            let task_id = task.task_id.clone();
            word.id(SharedString::from(format!("confirm-{}", task.task_id)))
                .text_color(accent)
                .cursor_pointer()
                .child("confirm")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    // The row under it opens the card; this click only confirms.
                    cx.stop_propagation();
                    ws.confirm_task(&task_id, cx)
                }))
                .into_any_element()
        }
        false => word
            .text_color(color)
            .child(task.status.name())
            .into_any_element(),
    };
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
    // A click on the row opens the task's card.
    let card = task.task_id.clone();
    div()
        .id(SharedString::from(format!("plan-row-{}", task.task_id)))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(ROW))
        .px_2()
        .cursor_pointer()
        .hover(|row| row.bg(cx.theme().list_hover))
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_task_card(&card, cx)))
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
                .child(task.title()),
        )
        .children(owner)
        .child(word)
}
