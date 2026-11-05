//! Where the shown chat works: the project and its folder, the branch, the model, the
//! mode and the approval policy.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, secondary) = (theme.muted_foreground, theme.foreground.opacity(0.8));
    let (name, dir) = match ws.selected_project() {
        Some(project) => (project.name.clone(), project.dir.clone()),
        None => ("No project".to_owned(), ws.home.clone()),
    };
    let branch = ws
        .git
        .as_ref()
        .and_then(|git| git.branch.clone())
        .unwrap_or_else(|| "no repository".to_owned());
    let line = |icon: IconName, text: String| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_4()
            .py_1()
            .text_sm()
            .text_color(secondary)
            .child(widgets::icon(icon, muted))
            .child(div().flex_1().min_w_0().truncate().child(text))
    };
    div()
        .flex()
        .flex_col()
        .child(line(IconName::Folder, name))
        .child(line(IconName::HardDrive, short_path(&dir)))
        .child(line(IconName::Github, branch))
        .child(line(
            IconName::Cpu,
            ws.status
                .model
                .clone()
                .unwrap_or_else(|| "no model".to_owned()),
        ))
        .child(line(
            IconName::Bot,
            format!(
                "{} mode · {:?} approvals",
                ws.status.mode, ws.status.approvals
            )
            .to_lowercase(),
        ))
}

/// `~/code/app` for a folder under home.
fn short_path(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if shown.starts_with(&home) => format!("~{}", &shown[home.len()..]),
        _ => shown,
    }
}
