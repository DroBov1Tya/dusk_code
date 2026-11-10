//! The shown chat's subagents, one short row each: its drawn mark, name, role, what
//! it does now and how long it has run, and its task on one line.

use gpui_kit::{component::ActiveTheme, *};

use super::glyph;
use crate::{
    chat::transcript::{Agent, AgentState},
    widgets,
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat().filter(|chat| !chat.agents.is_empty()) else {
        return div().child(widgets::muted("No subagents in this chat yet.", cx));
    };
    let theme = cx.theme();
    let (accent, muted) = (theme.primary, theme.muted_foreground);
    let mut column = div().flex().flex_col().gap_1().px_2();
    for (thread_id, agent) in &chat.agents {
        let color = if agent.working { accent } else { muted };
        let open = thread_id.clone();
        column = column.child(
            div()
                .id(SharedString::from(format!("agent-chat-{thread_id}")))
                .cursor_pointer()
                .hover(|row| row.bg(theme.accent))
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&open, cx)))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .child(glyph::render(thread_id, color))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .child(summary_line(agent, color, muted))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .truncate()
                                .child(agent.task.clone()),
                        ),
                ),
        );
    }
    column
}

/// `Ember-0c  general · thinking · 01:24`.
pub fn summary_line(agent: &Agent, color: Hsla, muted: Hsla) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_xs()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(agent.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(muted)
                .child(format!(
                    "{} \u{00B7} {}",
                    agent.role,
                    state_label(&agent.state)
                )),
        )
        .child(
            div()
                .flex_none()
                .text_color(muted)
                .child(clock(agent.run_time())),
        )
}

pub fn state_label(state: &AgentState) -> String {
    match state {
        AgentState::Thinking => "thinking".into(),
        AgentState::Writing => "writing".into(),
        AgentState::Tool(name) => name.clone(),
        AgentState::Done => "done".into(),
        AgentState::Failed => "failed".into(),
    }
}

/// `01:24`, or `1:02:03` past an hour.
fn clock(time: std::time::Duration) -> String {
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
