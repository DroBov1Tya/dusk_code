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

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

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
                .child(div().meta().text_color(muted).child(count.to_string()))
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
            .tooltip("Read the files again")
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
    let mut grid = div().grid().grid_cols(5).gap_4();
    for (index, file) in files.iter().enumerate() {
        grid = grid.child(card(index, file, cx));
    }
    let body = if files.is_empty() {
        crate::ui::empty(
            IconName::LayoutDashboard,
            "No files yet",
            "Files the agents write in the workspace appear here.",
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
    let menu_path = file.path.clone();
    div()
        .id(("artifact", index))
        .group("artifact")
        .relative()
        .flex()
        .flex_col()
        .rounded_2xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .overflow_hidden()
        .cursor_pointer()
        .hover(move |card| card.bg(raised))
        // A tile is a square window onto the file; the click opens it whole.
        .child(
            div()
                .w_full()
                .aspect_square()
                .overflow_hidden()
                .child(preview),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .child(div().small().truncate().child(name))
                .child(
                    div()
                        .meta()
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, event: &MouseDownEvent, _, cx| {
                ws.artifact_menu = Some((menu_path.clone(), event.position));
                cx.notify();
            }),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.view_file(path.clone(), cx)))
}

/// Tall enough for the four rows, the rule and the padding.
const MENU_HEIGHT: f32 = 168.;

/// What a right click on a tile offers: open, copy the path, show it in the system's file
/// manager, delete. Delete asks first.
pub fn menu(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let (path, at) = ws.artifact_menu.clone()?;
    // Near the bottom the menu opens upward, so it stays whole on screen.
    let room = window.viewport_size().height - at.y;
    let top = if room < px(MENU_HEIGHT + 8.) {
        at.y - px(MENU_HEIGHT)
    } else {
        at.y
    };
    let theme = cx.theme();
    let (muted, danger, border) = (theme.muted_foreground, theme.danger, theme.border);
    let (open, copy, reveal, delete) = (path.clone(), path.clone(), path.clone(), path);
    Some(
        div()
            .id("artifact-menu")
            .absolute()
            .left(at.x)
            .top(top)
            .occlude()
            .w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(theme.popover)
            .shadow_lg()
            .child(
                widgets::menu_item("artifact-menu-open", cx)
                    .child(widgets::icon(IconName::Eye, muted))
                    .child("Open")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.artifact_menu = None;
                        ws.view_file(open.clone(), cx);
                    })),
            )
            .child(
                widgets::menu_item("artifact-menu-copy", cx)
                    .child(widgets::icon(IconName::Copy, muted))
                    .child("Copy path")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.artifact_menu = None;
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            copy.display().to_string(),
                        ));
                        cx.notify();
                    })),
            )
            .child(
                widgets::menu_item("artifact-menu-reveal", cx)
                    .child(widgets::icon(IconName::FolderOpen, muted))
                    .child(if cfg!(target_os = "macos") {
                        "Show in Finder"
                    } else {
                        "Show in files"
                    })
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.artifact_menu = None;
                        cx.reveal_path(&reveal);
                        cx.notify();
                    })),
            )
            // What cannot be undone stands apart.
            .child(div().h(px(1.)).mx_2().my_1().bg(border))
            .child(
                widgets::menu_item("artifact-menu-delete", cx)
                    .text_color(danger)
                    .child(widgets::icon(IconName::Delete, danger))
                    .child("Delete")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.artifact_menu = None;
                        ws.modal = Some(crate::modal::Modal::DeleteFile {
                            path: delete.clone(),
                        });
                        cx.notify();
                    })),
            )
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.artifact_menu = None;
                cx.notify();
            })),
    )
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
