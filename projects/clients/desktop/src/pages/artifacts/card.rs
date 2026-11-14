//! One file as a tile: a thumbnail or an icon, its name, folder and size.

use dusk_protocol::project::{OutputFile, OutputKind, OutputPreview};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

pub fn card(index: usize, file: &OutputFile, cx: &mut Context<Workspace>) -> Stateful<Div> {
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
    let image = file.kind == OutputKind::Image;
    let icon = match file.kind {
        OutputKind::Image => IconName::LayoutDashboard,
        OutputKind::Text => IconName::FileText,
        OutputKind::Document => IconName::BookOpen,
        _ => IconName::File,
    };
    let placeholder = move || {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(widgets::icon(icon.clone(), muted))
            .into_any_element()
    };
    let source = match &file.preview {
        OutputPreview::Original if image => Some(&file.path),
        OutputPreview::Thumbnail(path) if image => Some(path),
        _ => None,
    };
    let preview = if let Some(source) = source {
        img(source.clone())
            .size_full()
            .object_fit(ObjectFit::Contain)
            .with_loading(placeholder.clone())
            .with_fallback(placeholder)
            .into_any_element()
    } else {
        placeholder()
    };
    let path = file.path.clone();
    let doomed = file.path.clone();
    let menu_path = file.path.clone();
    div()
        .id(("artifact", index))
        .focusable_row(cx)
        .group("artifact")
        .relative()
        .flex()
        .when(image, |tile| tile.w(px(164.)).flex_col())
        .when(!image, |tile| tile.w(px(250.)))
        .max_w_full()
        .items_center()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .overflow_hidden()
        .cursor_pointer()
        .hover(move |card| card.bg(raised))
        .child(
            if image {
                div().w_full().h(px(108.)).bg(theme.background)
            } else {
                div()
                    .size(px(36.))
                    .ml_2()
                    .flex_none()
                    .rounded_md()
                    .bg(theme.background)
            }
            .overflow_hidden()
            .child(preview),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .w_full()
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
                        .small()
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

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}
