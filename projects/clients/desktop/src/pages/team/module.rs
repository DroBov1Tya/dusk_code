//! The right column's Team screen: how the team stands, who works now and what waits for
//! the user, the plan in waves, and the members folded into one line of faces.

use dusk_protocol::thread::{TaskStatus, find_task};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use super::waves;
use crate::{
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// The faces the members' line shows before it says how many more.
const FACES: usize = 6;

/// Whether the module has something to show: the shown chat leads a team.
pub fn has_content(ws: &Workspace) -> bool {
    ws.shown_chat().is_some_and(|chat| chat.team.is_some())
}

/// "2 running · 1 needs you" for the column's list; how it ended once it did.
pub fn summary(ws: &Workspace) -> Option<String> {
    let chat = ws.shown_chat()?;
    let team = chat.team.as_ref()?;
    if team.ended().is_some() {
        return Some(team.progress_note());
    }
    let need = chat
        .tasks
        .iter()
        .filter(|task| waves::for_user(task))
        .count();
    Some(match need {
        0 => format!("{} of {} at work", team.at_work(), team.members.len()),
        need => format!("{} running \u{00B7} {need} needs you", team.at_work()),
    })
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some((chat_id, chat)) = ws.shown.as_deref().zip(ws.shown_chat()) else {
        return div();
    };
    let Some(team) = &chat.team else {
        return div();
    };
    let stop_button = team.ended().is_none().then(|| stop("team-stop-screen", cx));
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let label = |text: &'static str| div().meta().text_color(muted).pb_1().child(text);
    let running = team.ended().is_none();
    // How it runs and the way to stop it, then one line of how the plan stands.
    let head = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .child(widgets::capitalize(&team.progress_note())),
                )
                .children(stop_button),
        )
        .child(waves::summary(&chat.tasks, cx));
    let mut now = Vec::new();
    // One line a member at work: its name, its task's words, how long it has run.
    for member in team
        .members
        .iter()
        .filter(|member| running && member.status.at_work())
    {
        let task = find_task(&chat.tasks, &member.task_id).map(|(_, task)| task.title());
        let since = chat.agents.get(&member.thread_id).map(waves::running_for);
        let thread = member.thread_id.clone();
        now.push(
            now_line(
                SharedString::from(format!("team-now-{}", member.thread_id)),
                &member.name,
                task.unwrap_or_default(),
                true,
                cx,
            )
            .children(since.map(|since| div().flex_none().meta().text_color(muted).child(since)))
            .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&thread, cx))),
        );
    }
    // One line for each task that waits for the user: Review unfolds it in the plan below.
    for task in chat.tasks.iter().filter(|task| waves::for_user(task)) {
        let number = find_task(&chat.tasks, &task.task_id).map_or(0, |(number, _)| number);
        let who = task
            .owner
            .as_ref()
            .map_or_else(|| "A member".to_owned(), |owner| owner.name.clone());
        let (chat, id) = (chat_id.to_owned(), task.task_id.clone());
        now.push(
            now_line(
                SharedString::from(format!("team-need-{}", task.task_id)),
                &who,
                format!("wants your confirm on #{number}"),
                false,
                cx,
            )
            .child(
                div()
                    .flex_none()
                    .small()
                    .text_color(theme.primary)
                    .child("Review"),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                let tasks = ws
                    .shown_chat()
                    .map(|chat| chat.tasks.as_slice())
                    .unwrap_or_default();
                if let Some(wave) = waves::wave_of(tasks, &id) {
                    ws.team_page.waves.show_task(&chat, wave, &id);
                    cx.notify();
                }
            })),
        );
    }
    let now = (!now.is_empty()).then(|| div().flex().flex_col().child(label("Now")).children(now));
    let plan = (!chat.tasks.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .child(label("Plan").px_4())
            .child(waves::render(
                ws,
                waves::View {
                    key: "team",
                    folds: &ws.team_page.waves,
                    of: |ws| &mut ws.team_page.waves,
                    came: None,
                    inset: 40.,
                    lines: false,
                },
                cx,
            ))
    });
    div()
        .flex()
        .flex_col()
        .gap_6()
        .pt_2()
        .child(div().px_4().child(head))
        .children(now.map(|now| now.px_4()))
        .children(plan)
        .child(div().px_4().child(members(ws, cx)))
}

/// A line of the Now block: a dot, the member's name and what it does, in one line.
fn now_line(id: SharedString, name: &str, what: String, working: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let dot = div().flex_none().size(px(6.)).rounded_full();
    let dot = match working {
        true => waves::breathe(dot.bg(theme.primary)).into_any_element(),
        false => dot
            .border_1()
            .border_color(theme.primary)
            .into_any_element(),
    };
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(30.))
        .mx(px(-8.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .focusable_row(cx)
        .hover(move |line| line.bg(hover))
        .small()
        .child(dot)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .gap_1()
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.foreground.opacity(0.85))
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(format!("\u{00B7} {what}")),
                ),
        )
}

/// The small square button that asks before the team is stopped.
pub fn stop(id: &'static str, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (rest, over, mark) = (
        theme.foreground.opacity(0.06),
        theme.foreground.opacity(0.12),
        theme.foreground.opacity(0.7),
    );
    div()
        .id(id)
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rest)
        .cursor_pointer()
        .hover(move |stop| stop.bg(over))
        .tooltip(|window, cx| {
            gpui_kit::component::tooltip::Tooltip::new("Stop team").build(window, cx)
        })
        .child(div().size(px(8.)).rounded(px(2.)).bg(mark))
        .on_click(cx.listener(|ws, _, _, cx| {
            // A click on what holds it does something else; this one only asks to stop.
            cx.stop_propagation();
            ws.ask_stop_team(cx)
        }))
}

/// The members as one line of faces and their count; unfolded, a row each with its role,
/// model and state. A click on a member opens its chat.
fn members(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat() else {
        return div();
    };
    let Some(team) = &chat.team else {
        return div();
    };
    let theme = cx.theme();
    let (muted, hover, panel) = (theme.muted_foreground, theme.list_hover, theme.sidebar);
    let open = ws.team_page.members_open;
    let rows = team.member_rows(&chat.tasks);
    let mut faces = div().flex().flex_row();
    for (index, row) in rows.iter().take(FACES).enumerate() {
        faces = faces.child(
            div()
                .when(index > 0, |face| face.ml(px(-5.)))
                .rounded_full()
                .border_2()
                .border_color(panel)
                .child(waves::face(&row.name, 18., cx)),
        );
    }
    let line = div()
        .id("team-members")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(32.))
        .mx(px(-8.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .hover(move |line| line.bg(hover))
        .child(faces)
        .child(
            div()
                .flex_1()
                .small()
                .text_color(muted)
                .child(dusk_protocol::thread::plural(rows.len() as u32, "member")),
        )
        .child(widgets::icon(
            if open {
                gpui_kit::component::IconName::ChevronDown
            } else {
                gpui_kit::component::IconName::ChevronRight
            },
            muted,
        ))
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.team_page.members_open = !ws.team_page.members_open;
            cx.notify();
        }));
    let mut block = div().flex().flex_col().child(line);
    if open {
        let ended = team.ended().is_some();
        let mut list = div().flex().flex_col().pt_1();
        for row in rows {
            let role = [Some(row.role.clone()), row.model.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" \u{00B7} ");
            let state = match row.status {
                // A team that ended has nobody at work, whatever a task last said.
                status if ended && status.at_work() => "stopped",
                TaskStatus::Running => "running",
                status => status.member_word(),
            };
            let thread = row.thread_id.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("team-member-{}", row.thread_id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .py_1p5()
                    .mx(px(-8.))
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |row| row.bg(hover))
                    .child(waves::face(&row.name, 18., cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .small()
                                    .text_color(theme.foreground.opacity(0.85))
                                    .child(row.name.clone()),
                            )
                            .child(div().meta().text_color(muted).truncate().child(role)),
                    )
                    .child(div().flex_none().meta().text_color(muted).child(state))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&thread, cx))),
            );
        }
        block = block.child(widgets::unfold("team-members-list", list));
    }
    block
}

/// "Team ›" at the end of the plan card's title of a chat that leads a team: the way to
/// the page. The card's own click folds it; this one only opens the page.
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
