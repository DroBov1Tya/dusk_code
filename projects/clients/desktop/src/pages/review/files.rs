//! The changed files on the left, each with its added and removed lines.

use gpui_kit::{component::ActiveTheme, *};

use crate::{
    columns::inspector::git::{change_color, change_letter},
    ui::{Type, widgets},
    workspace::Workspace,
};

const FILES_WIDTH: f32 = 300.;

/// "3 files  +120  -8", the added in green and the removed in red.
pub fn stat(files: usize, added: u64, removed: u64, muted: Hsla, ok: Hsla, bad: Hsla) -> Div {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .small()
        .child(
            div()
                .text_color(muted)
                .child(format!("{files} file{}", if files == 1 { "" } else { "s" })),
        )
        .child(div().text_color(ok).child(format!("+{added}")))
        .child(div().text_color(bad).child(format!("\u{2212}{removed}")))
}

pub fn files(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (border, muted, success, danger) = (
        theme.border,
        theme.muted_foreground,
        theme.success,
        theme.danger,
    );
    let mut column = div()
        .id("review-files")
        .w(px(FILES_WIDTH))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .border_r_1()
        .border_color(border)
        .py_2();
    for (index, file) in ws.review.files.iter().enumerate() {
        let (folder, name) = match file.path.rsplit_once('/') {
            Some((folder, name)) => (format!("{folder}/"), name.to_owned()),
            None => (String::new(), file.path.clone()),
        };
        column = column.child(
            widgets::row(("review-file", index), index == ws.review.selected, cx)
                .gap_2()
                .child(
                    div()
                        .w(px(12.))
                        .meta()
                        .text_color(change_color(file.kind, cx))
                        .child(change_letter(file.kind)),
                )
                .child(
                    div().flex_1().min_w_0().truncate().small().child(
                        div()
                            .flex()
                            .flex_row()
                            .child(div().text_color(muted).child(folder))
                            .child(name),
                    ),
                )
                .child(div().meta().text_color(success).child(if file.added > 0 {
                    format!("+{}", file.added)
                } else {
                    String::new()
                }))
                .child(div().meta().text_color(danger).child(if file.removed > 0 {
                    format!("\u{2212}{}", file.removed)
                } else {
                    String::new()
                }))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.review.select(index);
                    cx.notify();
                })),
        );
    }
    column
}
