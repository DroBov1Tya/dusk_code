//! What `...` on a card offers.

use crate::{ui::widgets, workspace::Workspace};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

/// What `...` on a card offers: the file in the system's file manager, and its path.
pub fn menu(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let (path, at) = ws.audio.menu.clone()?;
    const HEIGHT: f32 = 84.;
    let room = window.viewport_size().height - at.y;
    let top = if room < px(HEIGHT + 8.) {
        at.y - px(HEIGHT)
    } else {
        at.y
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let (reveal, copy) = (path.clone(), path);
    Some(
        div()
            .id("audio-menu")
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
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(
                widgets::menu_item("audio-menu-reveal", cx)
                    .child(widgets::icon(IconName::FolderOpen, muted))
                    .child(if cfg!(target_os = "macos") {
                        "Show in Finder"
                    } else {
                        "Show in files"
                    })
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.audio.menu = None;
                        cx.reveal_path(&reveal);
                        cx.notify();
                    })),
            )
            .child(
                widgets::menu_item("audio-menu-copy", cx)
                    .child(widgets::icon(IconName::Copy, muted))
                    .child("Copy path")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.audio.menu = None;
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            copy.display().to_string(),
                        ));
                        cx.notify();
                    })),
            )
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.audio.menu = None;
                cx.notify();
            })),
    )
}
