//! What the shown chat did: tool runs by tool, files written, agents asked.

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat() else {
        return div();
    };
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    let activity = chat.activity();
    let runs: usize = activity.iter().map(|(_, count)| count).sum();
    let stat = |value: String, label: &'static str| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(foreground)
                    .child(value),
            )
            .child(div().text_xs().text_color(muted).child(label))
    };
    let mut column = div().flex().flex_col().gap_2().px_4().child(
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(stat(runs.to_string(), "tool runs"))
            .child(stat(chat.outputs.len().to_string(), "files written"))
            .child(stat(chat.agents.len().to_string(), "agents asked")),
    );
    for (name, count) in activity.iter().take(8) {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_xs()
                .child(div().font_family("Menlo").child(name.clone()))
                .child(div().text_color(muted).child(count.to_string())),
        );
    }
    column
}
