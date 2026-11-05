//! The files the shown chat wrote or changed, newest first; one opens in the viewer.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let outputs = ws.shown_chat().map(|chat| chat.outputs.as_slice()).unwrap_or_default();
    if outputs.is_empty() {
        return div().child(widgets::muted("Files the chat writes show here.", cx));
    }
    let muted = cx.theme().muted_foreground;
    // Relative paths are the project's, as the tools resolve them.
    let base = ws.selected_project().map_or(ws.home.clone(), |project| project.dir.clone());
    let mut list = div().flex().flex_col();
    for (index, path) in outputs.iter().rev().enumerate() {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = base.join(path);
        list = list.child(
            widgets::row(("output", index), false, cx)
                .child(widgets::icon(IconName::FileText, muted))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .on_click(cx.listener(move |ws, _, _, cx| ws.view_file(target.clone(), cx))),
        );
    }
    list
}
