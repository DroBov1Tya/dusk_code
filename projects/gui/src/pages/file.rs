//! A file open in the middle column as an editor shows it, read-only: line numbers,
//! syntax colors, folding and search (Cmd+F) come from the code editor of the UI kit.
//! The core reads the text, only inside a saved project or the chat folders.

use std::path::{Path, PathBuf};

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Editor, EditorState, Position},
    },
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
            .child(Editor::new(editor).readonly(true).bordered(false).h_full())
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

impl Workspace {
    /// Opens `path` in the middle column, at `line` when given. The core reads it, so only
    /// a file of a project or of the chat folders opens.
    pub fn open_file(&mut self, path: PathBuf, line: Option<u32>, cx: &mut Context<Self>) {
        if self.center != Center::File {
            self.file.back = self.center;
        }
        self.file.shown = self.shown_name(&path);
        self.file.path = path.clone();
        self.file.load = Load::Loading;
        self.file.editor = None;
        self.center = Center::File;
        cx.notify();
        let asked = path.clone();
        self.core.call(
            cx,
            move |client| async move { client.projects().read_output(asked).await },
            move |ws, result, cx| {
                // A file opened since is not this one's answer.
                if ws.file.path != path {
                    return;
                }
                match result {
                    Ok(text) => ws.file.pending = Some((text, line)),
                    Err(error) => ws.file.load = Load::Failed(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }

    pub fn close_file(&mut self, cx: &mut Context<Self>) {
        self.center = self.file.back;
        cx.notify();
    }

    /// The file's path relative to the chosen project, or as it is.
    fn shown_name(&self, path: &Path) -> String {
        self.projects
            .iter()
            .filter(|project| Some(&project.id) == self.project.as_ref())
            .find_map(|project| path.strip_prefix(&project.dir).ok())
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Opens a path from the chat, `path` or `path:line`, relative to the project.
    pub fn open_path(&mut self, path: &str, cx: &mut Context<Self>) {
        let (name, line) = match path.rsplit_once(':') {
            Some((name, line)) => (name, line.parse().ok()),
            None => (path, None),
        };
        let dir = self.project_dir().unwrap_or_else(|| self.home.clone());
        self.open_file(dir.join(name), line, cx);
    }

    /// The chosen project's folder.
    pub fn project_dir(&self) -> Option<PathBuf> {
        self.projects
            .iter()
            .find(|project| Some(&project.id) == self.project.as_ref())
            .map(|project| project.dir.clone())
    }
}
