//! A file open in the middle column, to read and to change: line numbers, syntax colors,
//! folding and search (Cmd+F) come from the code editor of the UI kit. The core reads the
//! text and writes it back, only inside a saved project or the chat folders, and refuses
//! to write over a file that changed on disk since it was opened.

mod open;

use std::path::PathBuf;

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Editor, EditorState, InputEvent, Position},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::widgets,
    workspace::{Center, Workspace},
};

enum Load {
    Loading,
    Ready,
    Failed(String),
}

pub struct FileView {
    pub path: PathBuf,
    /// What the header shows: relative to the project when it is inside one.
    shown: String,
    load: Load,
    editor: Option<Entity<EditorState>>,
    /// Text and line to jump to, put in the editor on the next frame, which has the window.
    pending: Option<(String, Option<u32>)>,
    /// The text as it is on disk, and the stamp the core gave with it.
    saved: String,
    stamp: Option<dusk_protocol::project::FileStamp>,
    /// The editor's text differs from what is on disk.
    dirty: bool,
    saving: bool,
    /// Tells when the text changes.
    _changes: Option<Subscription>,
    lines: usize,
    /// Where Back goes.
    back: Center,
}

impl FileView {
    pub fn new() -> Self {
        Self {
            path: PathBuf::new(),
            shown: String::new(),
            load: Load::Loading,
            editor: None,
            pending: None,
            saved: String::new(),
            stamp: None,
            dirty: false,
            saving: false,
            _changes: None,
            lines: 0,
            back: Center::Chat,
        }
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    if let Some((text, line)) = ws.file.pending.take() {
        let language = ws
            .file
            .path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned())
            .unwrap_or_default();
        ws.file.lines = text.lines().count();
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language));
        ws.file.saved = text.clone();
        ws.file.dirty = false;
        // Compared with what is on disk, so undoing back to it reads as saved again.
        ws.file._changes = Some(cx.subscribe(&editor, |ws, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = editor.read(cx).value();
                ws.file.dirty = text.as_ref() != ws.file.saved.as_str();
                ws.file.lines = text.lines().count();
                cx.notify();
            }
        }));
        editor.update(cx, |editor, cx| {
            editor.set_value(text, window, cx);
            if let Some(line) = line {
                editor.set_cursor_position(Position::new(line.saturating_sub(1), 0), window, cx);
            }
        });
        ws.file.editor = Some(editor);
        ws.file.load = Load::Ready;
    }
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
    let path = ws.file.path.clone();
    let copied = ws.file.path.display().to_string();
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
            Button::new("file-back")
                .ghost()
                .small()
                .icon(IconName::ArrowLeft)
                .label("Back")
                .on_click(cx.listener(|ws, _, _, cx| ws.close_file(cx))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .child(ws.file.shown.clone()),
        )
        // Unsaved work says so beside the name, and has its two ways out next to it.
        .when(ws.file.dirty, |header| {
            header
                .child(div().text_xs().text_color(theme.warning).child("Unsaved"))
                .child(
                    Button::new("file-discard")
                        .ghost()
                        .small()
                        .label("Discard")
                        .tooltip("Go back to the file as it is on disk")
                        .on_click(cx.listener(|ws, _, _, cx| ws.discard_file(cx))),
                )
        })
        .child(
            Button::new("file-save")
                .primary()
                .small()
                .label(if ws.file.saving { "Saving..." } else { "Save" })
                .disabled(!ws.file.dirty || ws.file.saving)
                .tooltip("Save the file")
                .on_click(cx.listener(|ws, _, _, cx| ws.save_file(cx))),
        )
        .child(div().text_xs().text_color(muted).child(match ws.file.load {
            Load::Ready => widgets::count(ws.file.lines, "line"),
            _ => String::new(),
        }))
        .child(
            Button::new("file-copy")
                .ghost()
                .small()
                .icon(IconName::Copy)
                .tooltip("Copy the path")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                })),
        )
        .child(
            Button::new("file-system")
                .ghost()
                .small()
                .icon(IconName::ExternalLink)
                .tooltip("Open in the system")
                .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&path))),
        );
    let body = match (&ws.file.load, &ws.file.editor) {
        (Load::Ready, Some(editor)) => div()
            .flex_1()
            .min_h_0()
            .child(Editor::new(editor).bordered(false).h_full())
            .into_any_element(),
        (Load::Failed(error), _) => div()
            .flex_1()
            .child(widgets::muted(error.clone(), cx))
            .into_any_element(),
        _ => div()
            .flex_1()
            .child(widgets::muted("Reading the file...", cx))
            .into_any_element(),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(body)
}
