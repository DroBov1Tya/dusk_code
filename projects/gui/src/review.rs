//! Review of the project's changes in the middle column: the files on the left with their
//! added and removed lines, the chosen file's diff on the right with line numbers and
//! green, red and grey lines, as an editor shows it. The core reads and caps the diff.

use dusk_protocol::project::{ChangeKind, DiffKind, DiffLine, FileDiff};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    inspector::git::{change_color, change_letter},
    ui::Type,
    widgets,
    workspace::{Center, Workspace},
};

const FILES_WIDTH: f32 = 300.;
const NUMBER_WIDTH: f32 = 46.;

#[derive(Clone, PartialEq)]
enum Row {
    Hunk(String),
    Line(DiffLine),
}

pub struct Review {
    pub files: Vec<FileDiff>,
    pub selected: usize,
    pub loading: bool,
    rows: Vec<Row>,
    list: ListState,
}

impl Review {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected: 0,
            loading: false,
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
        }
    }

    /// The next file, or the one before it for -1; the first and last stay put.
    pub fn step(&mut self, delta: isize) {
        let next = self.selected.saturating_add_signed(delta);
        if next < self.files.len() {
            self.select(next);
        }
    }

    /// The rows of the chosen file, and the list measured anew.
    fn select(&mut self, index: usize) {
        self.selected = index.min(self.files.len().saturating_sub(1));
        self.rows = self
            .files
            .get(self.selected)
            .map(|file| {
                file.hunks
                    .iter()
                    .flat_map(|hunk| {
                        std::iter::once(Row::Hunk(hunk.header.clone()))
                            .chain(hunk.lines.iter().cloned().map(Row::Line))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.list.reset(self.rows.len());
    }
}

pub fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, success, danger) = (
        theme.border,
        theme.muted_foreground,
        theme.success,
        theme.danger,
    );
    let (added, removed): (u64, u64) = ws
        .review
        .files
        .iter()
        .fold((0, 0), |(a, r), file| (a + file.added, r + file.removed));
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(border)
        .child(
            Button::new("review-back")
                .ghost()
                .small()
                .icon(IconName::ArrowLeft)
                .label("Chat")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.center = Center::Chat;
                    cx.notify();
                })),
        )
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Changes"))
        .child(stat(
            ws.review.files.len(),
            added,
            removed,
            muted,
            success,
            danger,
        ))
        .child(div().flex_1())
        .child(
            Button::new("review-refresh")
                .ghost()
                .small()
                .icon(IconName::RefreshCw)
                .tooltip("Read the changes again")
                .on_click(cx.listener(|ws, _, _, cx| ws.open_review(cx))),
        );
    let body = if ws.review.files.is_empty() {
        let state = if ws.review.loading {
            widgets::muted("Reading the changes...", cx)
        } else {
            crate::ui::empty(
                IconName::Github,
                "Nothing to review",
                "The work tree matches the last commit.",
                cx,
            )
        };
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(state)
    } else {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .child(files(ws, cx))
            .child(diff(ws, cx))
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(body)
}

/// "3 files  +120  -8", the added in green and the removed in red.
fn stat(files: usize, added: u64, removed: u64, muted: Hsla, ok: Hsla, bad: Hsla) -> Div {
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

fn files(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
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

fn diff(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Div {
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
                    .xsmall()
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
    let mono = crate::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    // The same size as the viewer's code, so a file reads alike in both.
    let code_size = crate::theme::prefs(cx).mono_size;
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

impl Workspace {
    /// Opens the review of the chosen project's changes and reads them.
    pub fn open_review(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        self.center = Center::Review;
        self.review.loading = true;
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.projects().diff(&id, None).await },
            |ws, result, cx| {
                ws.review.loading = false;
                match result {
                    Ok(files) => {
                        let keep = ws
                            .review
                            .files
                            .get(ws.review.selected)
                            .map(|file| file.path.clone());
                        ws.review.files = files;
                        let at = keep
                            .and_then(|path| ws.review.files.iter().position(|f| f.path == path))
                            .unwrap_or(0);
                        ws.review.select(at);
                        cx.notify();
                    }
                    Err(error) => ws.fail(error, cx),
                }
            },
        );
    }
}
