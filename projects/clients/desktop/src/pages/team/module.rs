//! The right column's Team module: the team in rows. How it stands and its steps, the
//! lead, a row for each member still in play on a rail from the lead, a line for the
//! rest, the plan's waves as columns of glyphs, and the way to the page.

use dusk_protocol::thread::{
    Team, TeamEnd, Work, WorkKind,
    team_layout::{self, EdgeTone, Ends, Lit, MemberNode, People, Pick, Related},
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use super::graph;
use crate::{
    pages::chat::activity,
    ui::{Type, widgets},
    workspace::Workspace,
};

const ROW_H: f32 = 42.;
/// Where the rail runs, from the row's left edge.
const RAIL_X: f32 = 10.;

/// Whether the module has something to show: the shown chat leads a team.
pub fn has_content(ws: &Workspace) -> bool {
    ws.shown_chat().is_some_and(|chat| chat.team.is_some())
}

/// "2 of 6 at work" for the folded header.
pub fn summary(ws: &Workspace) -> Option<String> {
    let team = ws.shown_chat()?.team.as_ref()?;
    Some(match team.ended() {
        Some(_) => team.state_word().to_owned(),
        None => format!("{} of {} at work", team.at_work(), team.members.len()),
    })
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat() else {
        return div();
    };
    let Some(team) = &chat.team else {
        return div();
    };
    let theme = cx.theme();
    let (foreground, success, warning, border, muted) = (
        theme.foreground,
        theme.success,
        theme.warning,
        theme.border,
        theme.muted_foreground,
    );
    let layout = team_layout::people(team, &chat.tasks, &chat.agents);
    let related = Related::of(ws.team_page.pick(), Some(team), &chat.tasks);
    let status = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(22.))
        .small()
        .child(lead_mark(team, cx))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(match team.ended() {
                    None => foreground.opacity(0.8),
                    Some(TeamEnd::Done) => success,
                    Some(_) => warning,
                })
                .child(team.progress_note()),
        );
    let [steps, ..] = team.budget_parts();
    let fill = match steps.near() {
        true => warning,
        false => foreground.opacity(0.7),
    };
    let budget = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(20.))
        .meta()
        .text_color(muted)
        .child("steps")
        .child(
            div()
                .flex_1()
                .h(px(3.))
                .rounded_full()
                .bg(border)
                .overflow_hidden()
                .child(div().h_full().w(relative(steps.share())).bg(fill)),
        )
        .child(steps.text());
    let lead = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(26.))
        .pl(px(2.))
        .small()
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .child("Lead"),
        )
        .child(
            div()
                .ml_auto()
                .min_w_0()
                .meta()
                .text_color(muted)
                .truncate()
                .child(match team.ended() {
                    Some(_) => layout.lead_tasks.clone(),
                    None => layout.lead_line.clone(),
                }),
        );
    let rows: Vec<&MemberNode> = layout.active().collect();
    let count = rows.len();
    let rows = rows
        .into_iter()
        .enumerate()
        .map(|(index, member)| {
            let lit = related
                .as_ref()
                .map(|related| related.member(&member.thread_id));
            row(member, &layout, index + 1 == count, lit, cx)
        })
        .collect::<Vec<_>>();
    let rest = layout
        .rest()
        .map(|rest| div().pl(px(4.)).pt_1().meta().text_color(muted).child(rest));
    let done = chat
        .tasks
        .iter()
        .filter(|task| task.status == dusk_protocol::thread::TaskStatus::Done)
        .count();
    let foot = div()
        .pt_2()
        .flex()
        .flex_row()
        .items_center()
        .meta()
        .text_color(muted)
        .child(format!(
            "{done} of {} tasks done \u{00B7} a column is a wave",
            chat.tasks.len()
        ))
        .child(
            div()
                .id("team-module-open")
                .ml_auto()
                .text_color(foreground.opacity(0.8))
                .cursor_pointer()
                .hover(|open| open.text_color(cx.theme().foreground))
                .child("Open \u{203A}")
                .on_click(cx.listener(|ws, _, _, cx| ws.open_team(cx))),
        );
    div()
        .px_4()
        .flex()
        .flex_col()
        // A click anywhere else drops the pick; one inside an open window does not.
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            if ws.modal.is_none() {
                ws.team_unpick(cx);
            }
        }))
        .child(status)
        .child(budget)
        .child(lead)
        .children(rows)
        .children(rest)
        .child(waves(&chat.tasks, related.as_ref(), cx))
        .child(foot)
}

/// The lead's mark: the agents' dots while the team runs, how it ended after.
fn lead_mark(team: &Team, cx: &App) -> Div {
    match team.ended() {
        None => activity::mark(
            &Work {
                kind: WorkKind::Agents {
                    working: team.at_work() as u32,
                    started: team.members.len() as u32,
                },
                label: String::new(),
                stalled: false,
            },
            cx,
        ),
        Some(TeamEnd::Done) => graph::glyph(dusk_protocol::thread::TaskStatus::Done, cx),
        Some(_) => graph::glyph(dusk_protocol::thread::TaskStatus::Blocked, cx),
    }
}

/// A member on the rail: its mark, name, task number and word, and under them what it
/// waits on, does or spent. A click opens its chat.
fn row(
    member: &MemberNode,
    layout: &People,
    last: bool,
    lit: Option<Lit>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover, accent) = (theme.muted_foreground, theme.list_hover, theme.primary);
    let tone = layout
        .edges
        .iter()
        .find(|edge| edge.ends == Ends::Lead(member.thread_id.clone()))
        .map_or(EdgeTone::Line, |edge| edge.tone);
    let rail = match tone {
        EdgeTone::Accent => accent,
        EdgeTone::Warning => theme.warning,
        EdgeTone::Danger => theme.danger.opacity(0.65),
        EdgeTone::Line => theme.foreground.opacity(0.3),
        EdgeTone::Faint => theme.foreground.opacity(0.11),
    };
    let word_color = widgets::tone(member.state.status().tone(), cx);
    let pick = Pick::Member(member.thread_id.clone());
    let (over, open) = (pick.clone(), pick);
    div()
        .id(SharedString::from(format!("team-row-{}", member.thread_id)))
        .relative()
        .h(px(ROW_H))
        .pl(px(22.))
        .flex()
        .flex_col()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .when(lit == Some(Lit::Picked), |row| {
            row.border_1().border_color(accent)
        })
        .when(lit == Some(Lit::Dim), |row| row.opacity(0.38))
        // The rail from the lead: down the gutter, then across to this row.
        .child(
            div()
                .absolute()
                .left(px(RAIL_X))
                .top(px(-4.))
                .w(px(1.))
                .h(px(if last { 16. } else { ROW_H + 4. }))
                .bg(theme.foreground.opacity(0.3)),
        )
        .child(
            div()
                .absolute()
                .left(px(RAIL_X))
                .top(px(12.))
                .w(px(9.))
                .h(px(1.))
                .bg(rail),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .small()
                .child(graph::member_mark(&member.state, cx))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .child(member.name.clone()),
                )
                .children(
                    member
                        .number
                        .map(|n| div().meta().text_color(muted).child(format!("#{n}"))),
                )
                .child(
                    div()
                        .ml_auto()
                        .meta()
                        .text_color(word_color)
                        .child(member.word),
                ),
        )
        .child(
            div()
                .pl(px(24.))
                .meta()
                .text_color(muted)
                .truncate()
                .child(member.narrow.clone()),
        )
        .on_hover(
            cx.listener(move |ws, inside: &bool, _, cx| ws.team_hover(over.clone(), *inside, cx)),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.team_open(open.clone(), cx)))
}

/// The plan as columns of glyphs, a wave each; a click on one opens its task's card.
fn waves(
    tasks: &[dusk_protocol::thread::Task],
    related: Option<&Related>,
    cx: &mut Context<Workspace>,
) -> Div {
    let accent = cx.theme().primary;
    let mut columns = div().pt_2().flex().flex_row().flex_wrap().gap_2();
    for wave in team_layout::waves(tasks) {
        let mut column = div().flex().flex_col().gap_px();
        for at in wave {
            let task = &tasks[at];
            let lit = related.map(|related| related.task(&task.task_id));
            let pick = Pick::Task(task.task_id.clone());
            let (over, open) = (pick.clone(), pick);
            column = column.child(
                div()
                    .id(SharedString::from(format!("team-dot-{}", task.task_id)))
                    .w(px(14.))
                    .h(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(rems(0.66))
                    .text_color(widgets::tone(task.status.tone(), cx))
                    .when(lit == Some(Lit::Picked), |dot| {
                        dot.border_1().border_color(accent)
                    })
                    .when(lit == Some(Lit::Dim), |dot| dot.opacity(0.38))
                    .child(task.status.glyph())
                    .on_hover(cx.listener(move |ws, inside: &bool, _, cx| {
                        ws.team_hover(over.clone(), *inside, cx)
                    }))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.team_open(open.clone(), cx))),
            );
        }
        columns = columns.child(column);
    }
    columns
}

/// "Open ›" at the end of the plan strip of a chat that leads a team: the way to the
/// page. The strip's own click folds it; this one only opens the page.
pub fn open_link(team: bool, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    team.then(|| {
        div()
            .id("plan-team-open")
            .flex_none()
            .meta()
            .text_color(muted)
            .cursor_pointer()
            .hover(move |open| open.text_color(foreground))
            .child("Team \u{203A}")
            .on_click(cx.listener(|ws, _, _, cx| {
                cx.stop_propagation();
                ws.open_team(cx)
            }))
    })
}
