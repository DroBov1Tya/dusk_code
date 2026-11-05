//! The shown chat's subagents, each with its own drawn mark and what it works on.

use gpui_kit::{component::ActiveTheme, *};

use super::glyph;
use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat().filter(|chat| !chat.agents.is_empty()) else {
        return div().child(widgets::muted("No subagents in this chat yet.", cx));
    };
    let theme = cx.theme();
    let (accent, muted, success) = (theme.primary, theme.muted_foreground, theme.success);
    let mut column = div().flex().flex_col().gap_2().p_2();
    for (thread_id, agent) in &chat.agents {
        let color = if agent.working { accent } else { muted };
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .p_2()
                .rounded_lg()
                .bg(theme.muted)
                .child(glyph::render(thread_id, color))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(div().font_weight(FontWeight::MEDIUM).child(agent.name.clone()))
                                .child(widgets::badge(
                                    if agent.working { "working" } else { "done" },
                                    if agent.working { accent } else { success },
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .line_clamp(3)
                                .child(agent.task.clone()),
                        ),
                ),
        );
    }
    column
}
