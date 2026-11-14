//! The chosen file's diff: hunk headings and lines with their numbers and colors.

use dusk_protocol::project::{ChangeKind, DiffKind};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::Row;
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const NUMBER_WIDTH: f32 = 46.;

pub fn diff(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let Some(file) = ws.review.files.get(ws.review.selected) else {
        return div().flex_1();
    };
    let (open_path, open_line) = (
        file.path.clone(),
        file.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .find_map(|line| line.new),
    );
    let can_open = file.kind != ChangeKind::Deleted;
    let title = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .small()
        .child(file.path.clone())
        .child(div().meta().text_color(muted).child(match file.kind {
            ChangeKind::Untracked => "new file",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Added => "added",
            ChangeKind::Renamed => "renamed",
            ChangeKind::Conflicted => "conflict",
            ChangeKind::Modified => "modified",
        }))
        .child(div().flex_1())
        .when(can_open, |row| {
            row.child(
                Button::new("review-open-file")
                    .ghost()
                    .small()
                    .icon(IconName::File)
                    .label("Open file")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        if let Some(dir) = ws.project_dir() {
                            ws.open_file(dir.join(&open_path), open_line, cx);
                        }
                    })),
            )
        });
    let content = if ws.review.rows.is_empty() {
        let text = if file.binary {
            "Binary or too large to show."
        } else {
            "No lines to show."
        };
        div()
            .flex_1()
            .child(widgets::muted(text, cx))
            .into_any_element()
    } else {
        let list_state = ws.review.list.clone();
        list(
            list_state,
            cx.processor(|ws, ix: usize, _window, cx| row(ws, ix, cx)),
        )
        .flex_1()
        .into_any_element()
    };
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(title)
        .child(content)
}

fn row(ws: &Workspace, ix: usize, cx: &mut Context<Workspace>) -> AnyElement {
    let theme = cx.theme();
    let (muted, success, danger, surface) = (
        theme.muted_foreground,
        theme.success,
        theme.danger,
        theme.muted,
    );
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    // The same size as the viewer's code, so a file reads alike in both.
    let code_size = crate::ui::theme::prefs(cx).mono_size;
    let Some(row) = ws.review.rows.get(ix) else {
        return div().into_any_element();
    };
    let number = |value: Option<u32>| {
        div()
            .w(px(NUMBER_WIDTH))
            .flex_shrink_0()
            .pr_2()
            .text_right()
            .text_color(muted.opacity(0.7))
            .child(value.map(|n| n.to_string()).unwrap_or_default())
    };
    match row {
        Row::Hunk(header) => div()
            .w_full()
            .px_4()
            .pt_3()
            .pb_2()
            .bg(surface.opacity(0.6))
            .text_size(px(code_size))
            .font_family(mono)
            .text_color(muted)
            .child(header.clone())
            .into_any_element(),
        Row::Line(line) => {
            let (tint, sign, color) = match line.kind {
                DiffKind::Added => (Some(success), "+", success),
                DiffKind::Removed => (Some(danger), "\u{2212}", danger),
                DiffKind::Context => (None, " ", muted),
            };
            div()
                .w_full()
                .flex()
                .flex_row()
                .text_size(px(code_size))
                .font_family(mono)
                .when_some(tint, |row, tint| row.bg(tint.opacity(0.12)))
                .child(number(line.old))
                .child(number(line.new))
                .child(
                    div()
                        .w(px(14.))
                        .flex_shrink_0()
                        .text_color(color)
                        .child(sign),
                )
                .child(div().flex_1().min_w_0().pr_4().child(line.text.clone()))
                .into_any_element()
        }
    }
}
