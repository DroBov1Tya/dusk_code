//! Opening, saving and closing the file through the core, and the paths it shows.

use std::path::{Path, PathBuf};

use gpui_kit::*;

use super::Load;
use crate::workspace::{Center, Workspace};

impl Workspace {
    /// Opens `path` in the middle column, at `line` when given. The core reads it, so only
    /// a file of a project or of the chat folders opens.
    pub fn open_file(&mut self, path: PathBuf, line: Option<u32>, cx: &mut Context<Self>) {
        if self.unsaved_file(cx) {
            return;
        }
        if self.center != Center::File {
            self.file.back = self.center;
        }
        self.file.shown = self.shown_name(&path);
        self.file.path = path.clone();
        self.file.load = Load::Loading;
        self.file.editor = None;
        self.file._changes = None;
        self.file.stamp = None;
        self.file.dirty = false;
        self.center = Center::File;
        cx.notify();
        let asked = path.clone();
        self.core.call(
            cx,
            move |client| async move { client.projects().read_file(asked).await },
            move |ws, result, cx| {
                // A file opened since is not this one's answer.
                if ws.file.path != path {
                    return;
                }
                match result {
                    Ok(file) => {
                        ws.file.stamp = Some(file.stamp);
                        ws.file.pending = Some((file.text, line));
                    }
                    Err(error) => ws.file.load = Load::Failed(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }

    pub fn close_file(&mut self, cx: &mut Context<Self>) {
        if self.unsaved_file(cx) {
            return;
        }
        self.center = self.file.back;
        cx.notify();
    }

    /// Whether the open file has changes that leaving it would lose; says so when it has.
    fn unsaved_file(&mut self, cx: &mut Context<Self>) -> bool {
        if self.file.dirty {
            self.center = Center::File;
            self.fail(
                anyhow::anyhow!(
                    "{} has unsaved changes: save or discard them first",
                    self.file.shown
                ),
                cx,
            );
        }
        self.file.dirty
    }

    /// Writes the editor's text through the core. The stamp read with the file goes
    /// along, so a file something else changed meanwhile is not written over.
    pub fn save_file(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.file.editor.clone() else {
            return;
        };
        if !self.file.dirty || self.file.saving || self.center != Center::File {
            return;
        }
        let text = editor.read(cx).value().to_string();
        let (path, seen) = (self.file.path.clone(), self.file.stamp);
        let (asked, written) = (path.clone(), text.clone());
        self.file.saving = true;
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.projects().write_file(asked, text, seen).await },
            move |ws, result, cx| {
                // Another file opened since is not this one's answer.
                if ws.file.path != path {
                    return;
                }
                ws.file.saving = false;
                match result {
                    Ok(stamp) => {
                        ws.file.stamp = Some(stamp);
                        ws.file.dirty = ws
                            .file
                            .editor
                            .as_ref()
                            .is_some_and(|editor| editor.read(cx).value().as_ref() != written);
                        ws.file.saved = written;
                        ws.refresh_inspector(cx);
                    }
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
    }

    /// Reads the file again, dropping what was typed.
    pub fn discard_file(&mut self, cx: &mut Context<Self>) {
        self.file.dirty = false;
        let path = self.file.path.clone();
        self.open_file(path, None, cx);
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
