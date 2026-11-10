//! The chat's subagents by role, each with a small figure that shows what it does:
//! thinking, writing, a tool at work, done or failed. A row opens the agent's chat.

use std::collections::BTreeMap;

use gpui_kit::{component::ActiveTheme, *};

use crate::{
    chat::transcript::{Agent, AgentState},
    inspector::{agents::summary_line, mascot},
    widgets,
    workspace::Workspace,
};

pub fn render(ws: &Workspace, thread_id: &str, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws
        .chats
        .get(thread_id)
        .filter(|chat| !chat.agents.is_empty())
    else {
        return div()
            .p_5()
            .child(widgets::muted("This chat has asked no agents yet.", cx));
    };
    let theme = cx.theme();
    let (accent, muted, success, danger) = (
        theme.primary,
        theme.muted_foreground,
        theme.success,
        theme.danger,
    );
    let mut roles: BTreeMap<&str, Vec<(&String, &Agent)>> = BTreeMap::new();
    for (id, agent) in &chat.agents {
        roles
            .entry(agent.role.as_str())
            .or_default()
            .push((id, agent));
    }
    let mut column = div().flex().flex_col().gap_4().p_5();
    for (role, agents) in roles {
        let working = agents.iter().filter(|(_, agent)| agent.working).count();
        let mut group = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_xs()
                .text_color(muted)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(role.to_owned()),
                )
                .child(format!("{} \u{00B7} {working} at work", agents.len())),
        );
        for (id, agent) in agents {
            let color = match agent.state {
                AgentState::Done => success,
                AgentState::Failed => danger,
                _ => accent,
            };
            let open = id.clone();
            group = group.child(
                div()
                    .id(SharedString::from(format!("agents-row-{id}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(theme.accent))
                    .child(mascot::render(
                        SharedString::from(format!("mascot-{id}")),
                        &agent.state,
                        color,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(summary_line(agent, color, muted))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(agent.task.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&open, cx))),
            );
        }
        column = column.child(group);
    }
    column
}
