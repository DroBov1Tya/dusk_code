//! The files the agents made, in the middle column: by kind (text, pictures, documents
//! and the rest), newest first. Pictures show as thumbnails; any file opens in the viewer.

use dusk_protocol::project::{OutputFile, OutputKind};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{widgets, workspace::Workspace};

const KINDS: &[(Option<OutputKind>, &str)] = &[
    (None, "All"),
    (Some(OutputKind::Text), "Text"),
    (Some(OutputKind::Image), "Images"),
    (Some(OutputKind::Document), "Documents"),
    (Some(OutputKind::Other), "Other"),
];

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (foreground, muted, accent, border) = (
        theme.foreground,
        theme.muted_foreground,
        theme.primary,
        theme.border,
    );
    let mut tabs = div().flex().flex_row().items_center().gap_6().px_6().pt_5();
    for (kind, label) in KINDS {
        let kind = *kind;
        let count = ws
            .outputs
            .iter()
            .filter(|file| kind.is_none_or(|kind| file.kind == kind))
            .count();
        let active = ws.artifact_kind == kind;
        tabs = tabs.child(
            div()
                .id(*label)
                .flex()
                .flex_row()
                .items_baseline()
                .gap_1()
                .pb_1()
                .cursor_pointer()
                .border_b_2()
                .border_color(if active { accent } else { transparent_black() })
                .text_color(if active { foreground } else { muted })
                .child(*label)
                .child(div().text_xs().text_color(muted).child(count.to_string()))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.artifact_kind = kind;
                    cx.notify();
                })),
        );
    }
    tabs = tabs.child(div().flex_1()).child(
        Button::new("artifacts-refresh")
            .ghost()
            .small()
            .icon(IconName::RefreshCw)
            .on_click(cx.listener(|ws, _, _, cx| ws.load_outputs(cx))),
    );
    let files: Vec<OutputFile> = ws
        .outputs
        .iter()
        .filter(|file| ws.artifact_kind.is_none_or(|kind| file.kind == kind))
        .take(300)
        .cloned()
        .collect();
    if !files.is_empty() {
        let paths: Vec<_> = files.iter().map(|file| file.path.clone()).collect();
        tabs = tabs.child(
            Button::new("artifacts-delete-all")
                .ghost()
                .small()
                .icon(IconName::Delete)
                .tooltip("Delete every file shown")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(crate::modal::Modal::DeleteFiles {
                        paths: paths.clone(),
                    });
                    cx.notify();
                })),
        );
    }
    let mut grid = div().grid().grid_cols(3).gap_3();
    for (index, file) in files.iter().enumerate() {
        grid = grid.child(card(index, file, cx));
    }
    let body = if files.is_empty() {
        widgets::muted(
            "Nothing here yet. Files the agents write in the workspace show here.",
            cx,
        )
        .into_any_element()
    } else {
        grid.into_any_element()
    };
    div()
        .id("artifacts")
        .size_full()
        .overflow_y_scroll()
        .child(tabs)
        .child(div().h(px(1.)).mt_3().bg(border))
        .child(div().p_6().child(body))
}

fn card(index: usize, file: &OutputFile, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, surface, border, raised) = (
        theme.muted_foreground,
        theme.muted,
        theme.border,
        theme.accent,
    );
    let name = file
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = file
        .path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let preview = match file.kind {
        OutputKind::Image => img(file.path.clone())
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        kind => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(widgets::icon(
                match kind {
                    OutputKind::Text => IconName::FileText,
                    OutputKind::Document => IconName::BookOpen,
                    _ => IconName::File,
                },
                muted,
            ))
            .into_any_element(),
    };
    let path = file.path.clone();
    let doomed = file.path.clone();
    div()
        .id(("artifact", index))
        .group("artifact")
        .relative()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .overflow_hidden()
        .cursor_pointer()
        .hover(move |card| card.bg(raised))
        .child(div().h(px(120.)).child(preview))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .child(div().text_sm().truncate().child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(format!("{folder} · {}", size(file.bytes))),
                ),
        )
        .child(
            div()
                .absolute()
                .top_2()
                .right_2()
                .invisible()
                .group_hover("artifact", |corner| corner.visible())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(("artifact-delete", index))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Delete)
                        .tooltip("Delete")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            // The card under the button must not open the file.
                            cx.stop_propagation();
                            ws.modal = Some(crate::modal::Modal::DeleteFile {
                                path: doomed.clone(),
                            });
                            cx.notify();
                        })),
                ),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.view_file(path.clone(), cx)))
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}

impl Workspace {
    pub fn load_outputs(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().outputs().await },
            |ws, result, cx| match result {
                Ok(files) => {
                    ws.outputs = files;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }
}
