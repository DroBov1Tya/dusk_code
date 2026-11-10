//! The files the shown chat's commands changed in its project, as git saw them before
//! and after each command.

use gpui_kit::*;

use super::git::{change_color, change_letter};
use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut column = div().flex().flex_col();
    for change in ws.touched.iter().take(200) {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .px_3()
                .text_xs()
                .child(
                    div()
                        .w(px(12.))
                        .text_color(change_color(change.kind, cx))
                        .child(change_letter(change.kind)),
                )
                .child(div().truncate().child(change.path.clone())),
        );
    }
    column
}

impl Workspace {
    /// Reads the shown chat's touched files again.
    pub fn load_touched(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.work().changed_files(&thread_id).await },
            move |ws, result, cx| match result {
                Ok(changes) if ws.shown.as_deref() == Some(asked.as_str()) => {
                    ws.touched = changes;
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
