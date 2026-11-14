//! What a right click on a tile offers.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{ui::widgets, workspace::Workspace};

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
