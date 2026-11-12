//! The chat's subagents as a stage: each kind of work has its zone (engineering, security,
//! testing and review, infrastructure, the rest), and in it every agent stands as its 8-bit
//! figure with a bubble that says what it does this moment: thinking, writing, or the tool
//! it runs. A figure that works moves; one that is done rests. A click opens its chat.

use std::time::Duration;

use gpui_kit::{
    component::{ActiveTheme, Colorize, IconName},
    *,
};

use dusk_protocol::agent::Zone;

use crate::{
    columns::inspector::mascot,
    pages::chat::transcript::{Agent, AgentState},
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

pub fn render(ws: &Workspace, thread_id: &str, wide: bool, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws
        .chats
        .get(thread_id)
        .filter(|chat| !chat.agents.is_empty())
    else {
        return div().p_5().child(crate::ui::empty(
            IconName::Bot,
            "No subagents yet",
            "When the agent asks others for help, they appear here at work.",
            cx,
        ));
    };
    let theme = cx.theme();
    let muted = theme.foreground.mix_oklab(theme.muted_foreground, 0.16);
    let (signal, surface) = (theme.warning, theme.muted);
    let working = chat.agents.values().filter(|agent| agent.working).count();
    let mut zones: Vec<(Zone, Vec<(&String, &Agent)>)> =
        Zone::ALL.iter().map(|zone| (*zone, Vec::new())).collect();
    for (id, agent) in &chat.agents {
        let own = Zone::of(&agent.role);
        if let Some((_, members)) = zones.iter_mut().find(|(zone, _)| *zone == own) {
            members.push((id, agent));
        }
    }
    let mut stage = div().grid().grid_cols(if wide { 2 } else { 1 }).gap_3();
    for (name, mut members) in zones.into_iter().filter(|(_, members)| !members.is_empty()) {
        // Those at work first, then the oldest.
        members.sort_by_key(|(_, agent)| (!agent.working, agent.started));
        let busy = members.iter().filter(|(_, agent)| agent.working).count();
        let mut card = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_lg()
            .bg(surface)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .group_name()
                            .truncate()
                            .child(name.name()),
                    )
                    .child(
                        div()
                            .meta()
                            .text_color(if busy > 0 { signal } else { muted })
                            .child(if busy > 0 {
                                format!("{busy} active")
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
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .body()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(widgets::count(chat.agents.len(), "subagent")),
                )
                .child(
                    div()
                        .small()
                        .text_color(if working > 0 { signal } else { muted })
                        .child(format!("{working} active")),
                ),
        )
        .child(stage)
}

/// One agent: its figure, its name, and a bubble with what it does now.
fn figure(id: &str, agent: &Agent, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.foreground.mix_oklab(theme.muted_foreground, 0.16);
    let (signal, success, danger, hover) = (
        theme.warning,
        theme.success,
        theme.danger,
        theme.list_active,
    );
    let color = match agent.state {
        AgentState::Done => success.opacity(0.8),
        AgentState::Failed => danger,
        _ => signal,
    };
    let status = match &agent.state {
        AgentState::Thinking => "Thinking".to_owned(),
        AgentState::Writing => "Writing".to_owned(),
        AgentState::Tool(name) => format!("Running {name}"),
        AgentState::Done => "Done".to_owned(),
        AgentState::Failed => "Failed".to_owned(),
    };
    let open = id.to_owned();
    div()
        .id(SharedString::from(format!("agents-row-{id}")))
        .focusable_row(cx)
        .role(Role::Button)
        .aria_label(format!("Open {}", agent.name))
        .aria_description(status.clone())
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .p_2()
        .rounded_lg()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(mascot::render(
            SharedString::from(format!("mascot-{id}")),
            &agent.state,
            color,
            3.0,
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
                                .min_w_0()
                                .truncate()
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
                                .small()
                                .truncate()
                                .text_color(if agent.state == AgentState::Done {
                                    muted
                                } else {
                                    color
                                })
                                .child(status),
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
                        .text_color(muted)
                        .truncate()
                        .child(agent.task.clone()),
                ),
        )
        .child(widgets::icon(IconName::ChevronRight, muted))
        .on_click(cx.listener(move |ws, _, window, cx| {
            ws.open_agent_chat(&open, cx);
            ws.agents_modal_focus.focus(window, cx);
        }))
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
