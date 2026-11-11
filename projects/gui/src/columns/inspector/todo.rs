//! The shown chat's task board, as its agents keep it.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{ui::widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let tasks = ws
        .shown_chat()
        .map(|chat| chat.tasks.as_slice())
        .unwrap_or_default();
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
                .min_w_0()
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
                    .items_start()
                    .gap_2()
                    .px_3()
                    .text_xs()
                    // The text wraps inside the column instead of running past it.
                    .child(
                        div()
                            .flex_shrink_0()
                            .pt(px(1.))
                            .child(widgets::icon(icon, color)),
                    )
                    .child(div().flex_1().min_w_0().child(item.description.clone())),
            );
        }
    }
    column
}
