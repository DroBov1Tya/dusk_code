//! The chat's subagents as a stage: each kind of work has its zone (engineering, security,
//! testing and review, infrastructure, the rest), and in it every agent stands as its 8-bit
//! figure with a bubble that says what it does this moment: thinking, writing, or the tool
//! it runs. A figure that works moves; one that is done rests. A click opens its chat.

use std::time::Duration;

use gpui_kit::{component::ActiveTheme, *};

use dusk_protocol::agent::Zone;

use crate::{
    chat::transcript::{Agent, AgentState},
    inspector::mascot,
    ui::Type,
    widgets,
    workspace::Workspace,
};

pub fn render(ws: &Workspace, thread_id: &str, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws
        .chats
        .get(thread_id)
        .filter(|chat| !chat.agents.is_empty())
    else {
        return div().p_5().child(crate::ui::empty(
            gpui_kit::component::IconName::Bot,
            "No subagents yet",
            "When the agent asks others for help, they appear here at work.",
            cx,
        ));
    };
    let theme = cx.theme();
    let (muted, signal, surface, border) = (
        theme.muted_foreground,
        theme.warning,
        theme.muted,
        theme.border,
    );
    let working = chat.agents.values().filter(|agent| agent.working).count();
    let mut zones: Vec<(Zone, Vec<(&String, &Agent)>)> =
        Zone::ALL.iter().map(|zone| (*zone, Vec::new())).collect();
    for (id, agent) in &chat.agents {
        let own = Zone::of(&agent.role);
        if let Some((_, members)) = zones.iter_mut().find(|(zone, _)| *zone == own) {
            members.push((id, agent));
        }
    }
    let mut stage = div().grid().grid_cols(2).gap_3();
    for (name, mut members) in zones.into_iter().filter(|(_, members)| !members.is_empty()) {
        // Those at work first, then the oldest.
        members.sort_by_key(|(_, agent)| (!agent.working, agent.started));
        let busy = members.iter().filter(|(_, agent)| agent.working).count();
        let mut card = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .pb_1()
                    .child(div().group_name().child(name.name()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .meta()
                            .text_color(if busy > 0 { signal } else { muted })
                            .child(if busy > 0 {
                                format!("{busy} at work")
                            } else {
                                widgets::count(members.len(), "agent")
                            }),
                    ),
            );
        for (id, agent) in members {
            card = card.child(figure(id, agent, cx));
        }
        stage = stage.child(card);
    }
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .child(div().small().text_color(muted).child(match working {
            0 => format!("{} in this chat, none at work.", chat.agents.len()),
            working => format!(
                "{} at work of {}.",
                working,
                widgets::count(chat.agents.len(), "agent")
            ),
        }))
        .child(stage)
}

/// One agent: its figure, its name, and a bubble with what it does now.
fn figure(id: &str, agent: &Agent, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, signal, success, danger, hover) = (
        theme.muted_foreground,
        theme.warning,
        theme.success,
        theme.danger,
        theme.accent,
    );
    let color = match agent.state {
        AgentState::Done => success.opacity(0.8),
        AgentState::Failed => danger,
        _ => signal,
    };
    let open = id.to_owned();
    div()
        .id(SharedString::from(format!("agents-row-{id}")))
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .p_2()
        .rounded_lg()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(mascot::render(
            SharedString::from(format!("mascot-{id}")),
            &agent.state,
            color,
            4.0,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .small()
                                .font_weight(FontWeight::MEDIUM)
                                .child(agent.name.clone()),
                        )
                        .child(
                            div()
                                .meta()
                                .text_color(muted)
                                .truncate()
                                .child(agent.role.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(bubble(id, agent, color, muted)),
                        )
                        .child(
                            div()
                                .meta()
                                .flex_none()
                                .text_color(muted)
                                .child(clock(agent.run_time())),
                        ),
                )
                .child(
                    div()
                        .meta()
                        .text_color(muted.opacity(0.85))
                        .truncate()
                        .child(agent.task.clone()),
                ),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&open, cx)))
}

/// What it does this moment, in a small bubble that matches the action: thinking breathes,
/// a tool shows its name as code, writing and the end say so plainly.
fn bubble(id: &str, agent: &Agent, color: Hsla, muted: Hsla) -> Div {
    let base = div().small().truncate().text_color(color);
    match &agent.state {
        AgentState::Thinking => base.child(div().child("thinking\u{2026}").with_animation(
            SharedString::from(format!("think-{id}")),
            Animation::new(Duration::from_millis(1600)).repeat(),
            |text, phase| text.opacity(0.45 + 0.55 * (phase * std::f32::consts::TAU).sin().abs()),
        )),
        AgentState::Writing => base.child("writing"),
        AgentState::Tool(name) => base.child(format!("running {name}")),
        AgentState::Done => base.text_color(muted).child("done"),
        AgentState::Failed => base.child("failed"),
    }
}

/// `01:24`, or `1:02:03` past an hour.
fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    match seconds {
        0..3600 => format!("{:02}:{:02}", seconds / 60, seconds % 60),
        _ => format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
    }
}
