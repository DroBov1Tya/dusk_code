//! Review of the project's changes in the middle column: the files on the left with their
//! added and removed lines, the chosen file's diff on the right with line numbers and
//! green, red and grey lines, as an editor shows it. The core reads and caps the diff.

mod diff;
mod files;

use diff::diff;
use files::{files, stat};

use dusk_protocol::project::{DiffLine, FileDiff};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    ui::widgets,
    workspace::{Center, Workspace},
};

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
