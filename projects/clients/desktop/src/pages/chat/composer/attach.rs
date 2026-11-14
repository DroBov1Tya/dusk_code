//! What goes with the message: files, folders, pasted pictures and long pastes, shown as a
//! strip above the text, and the menu that adds them.

use crate::ui::Type;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::workspace::Workspace;

const THUMB: f32 = 64.;

/// What goes with the message, in a row above the text that scrolls sideways: pictures as
/// thumbnails, other files and long pastes as chips, each with a button that drops it.
pub fn strip(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    if ws.attachments.is_empty() && ws.pastes.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let (muted, raised, border) = (theme.muted_foreground, theme.accent, theme.border);
    let mut row = div()
        .id("attachments")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .pt_3()
        .overflow_x_scroll();
    for (index, path) in ws.attachments.iter().enumerate() {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let remove = Button::new(("unattach", index))
            .ghost()
            .small()
            .icon(IconName::Close)
            .tooltip("Remove the attachment")
            .on_click(cx.listener(move |ws, _, _, cx| {
                if index < ws.attachments.len() {
                    ws.attachments.remove(index);
                }
                cx.notify();
            }));
        row = row.child(if is_picture(path) {
            div()
                .relative()
                .flex_none()
                .size(px(THUMB))
                .rounded_xl()
                .overflow_hidden()
                .border_1()
                .border_color(border)
                .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().top_0().right_0().child(remove))
        } else {
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(THUMB))
                .pl_3()
                .rounded_xl()
                .bg(raised)
                .meta()
                .child(crate::ui::widgets::icon(
                    if path.is_dir() {
                        IconName::Folder
                    } else {
                        IconName::File
                    },
                    muted,
                ))
                .child(name)
                .child(remove)
        });
    }
    for (index, text) in ws.pastes.iter().enumerate() {
        row = row.child(
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(THUMB))
                .pl_3()
                .rounded_xl()
                .bg(raised)
                .meta()
                .child(crate::ui::widgets::icon(IconName::FileText, muted))
                .child(paste_label(text))
                .child(
                    Button::new(("unpaste", index))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Remove the pasted text")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if index < ws.pastes.len() {
                                ws.pastes.remove(index);
                            }
                            cx.notify();
                        })),
                ),
        );
    }
    Some(row)
}

fn is_picture(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
            )
        })
}

/// Pastes longer than this, in characters, become a chip, as in the terminal client.
pub const PASTE_THRESHOLD: usize = 1500;

/// `pasted text · 120 lines`.
pub fn paste_label(text: &str) -> String {
    format!(
        "pasted text \u{00B7} {}",
        crate::ui::widgets::count(text.lines().count(), "line")
    )
}

/// Files first, then folders; each opens the system's own picker.
pub fn attach_menu(has_chat: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id("attach-menu")
        .absolute()
        .occlude()
        .bottom(px(64.))
        .left(px(24.))
        .w(px(220.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .child(
            crate::ui::widgets::row("attach-files", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::File,
                    theme.muted_foreground,
                ))
                .child("Files")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(false, window, cx))),
        )
        .children(has_chat.then(|| {
            crate::ui::widgets::row("attach-plan", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::FileText,
                    theme.muted_foreground,
                ))
                .child("Plan")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.attach_open = false;
                    ws.edit_plan(cx);
                }))
        }))
        .child(
            crate::ui::widgets::row("attach-folders", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::Folder,
                    theme.muted_foreground,
                ))
                .child("Folders")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(true, window, cx))),
        )
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.attach_open = false;
            cx.notify();
        }))
}

impl Workspace {
    /// Writes a pasted picture next to the core's pictures, readable by this user only.
    pub fn keep_pasted_image(&self, image: &Image) -> anyhow::Result<std::path::PathBuf> {
        use anyhow::Context as _;
        let folder = self.status.artifacts_dir.join("pasted");
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(&folder)
            .context("cannot create the pasted pictures folder")?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let path = folder.join(format!("pasted-{stamp}.{}", image.format.extension()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options
            .open(&path)
            .context("cannot save the pasted picture")?;
        std::io::Write::write_all(&mut file, &image.bytes)
            .context("cannot save the pasted picture")?;
        Ok(path)
    }

    /// Adds the picked files or folders to the next message.
    pub fn attach(&mut self, folders: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.attach_open = false;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |ws, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = ws.update(cx, |ws, cx| {
                ws.attachments.extend(paths);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
