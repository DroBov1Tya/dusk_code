//! The shown chat's task board, as its agents keep it.

use gpui_kit::{component::{ActiveTheme, IconName}, *};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let tasks = ws.shown_chat().map(|chat| chat.tasks.as_slice()).unwrap_or_default();
    if tasks.is_empty() {
        return div().child(widgets::muted("No tasks in this chat.", cx));
    }
    let theme = cx.theme();
    let (done, open) = (theme.success, theme.muted_foreground);
    let mut column = div().flex().flex_col().gap_1().py_2();
    for task in tasks {
        column = column.child(
            div()
                .px_3()
                .pt_1()
                .font_weight(FontWeight::MEDIUM)
                .child(task.description.clone()),
        );
        for item in &task.items {
            let (icon, color) = if item.done {
                (IconName::CircleCheck, done)
            } else {
                (IconName::Square, open)
            };
            column = column.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .text_xs()
                    .child(widgets::icon(icon, color))
                    .child(item.description.clone()),
            );
        }
    }
    column
}
