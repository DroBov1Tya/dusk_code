//! The project's files as a tree that opens a folder at a time, colored by git.

use gpui_kit::{component::{ActiveTheme, IconName}, *};

use super::git::{change_color, change_letter};
use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    if ws.project.is_none() {
        return div().child(widgets::muted("Pick a project to see its files.", cx));
    }
    let mut column = div().flex().flex_col().py_1();
    folder(ws, "", 0, &mut column, cx);
    column
}

fn folder(ws: &Workspace, path: &str, depth: usize, column: &mut Div, cx: &mut Context<Workspace>) {
    let Some(entries) = ws.files.get(path) else {
        *column = std::mem::replace(column, div()).child(widgets::muted("Loading...", cx));
        return;
    };
    let muted = cx.theme().muted_foreground;
    for entry in entries {
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path).to_owned();
        let open = ws.expanded.contains(&entry.path);
        let icon = match (entry.is_dir, open) {
            (true, true) => IconName::FolderOpen,
            (true, false) => IconName::FolderClosed,
            (false, _) => IconName::File,
        };
        let color = entry.change.map_or(cx.theme().foreground, |kind| change_color(kind, cx));
        let target = entry.path.clone();
        let is_dir = entry.is_dir;
        let row = widgets::row(SharedString::from(format!("file-{}", entry.path)), false, cx)
            .pl(px(8. + depth as f32 * 14.))
            .child(widgets::icon(icon, muted))
            .child(div().flex_1().min_w_0().truncate().text_color(color).child(name))
            .children(
                entry
                    .change
                    .map(|kind| div().text_xs().text_color(color).child(change_letter(kind))),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                if is_dir {
                    ws.toggle_folder(target.clone(), cx);
                }
            }));
        *column = std::mem::replace(column, div()).child(row);
        if entry.is_dir && open {
            folder(ws, &entry.path, depth + 1, column, cx);
        }
    }
}

impl Workspace {
    fn toggle_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path.clone());
            self.load_folder(path, cx);
        }
        cx.notify();
    }

    pub fn load_folder(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move {
                let entries = client.projects().files(&id, &path).await?;
                anyhow::Ok((path, entries))
            },
            |ws, result, cx| match result {
                Ok((path, entries)) => {
                    ws.files.insert(path, entries);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
