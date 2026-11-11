//! Where the shown chat works: the project and its folder, the branch, the model, the
//! mode and the approval policy. A click on the folder opens what to do with it.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use std::path::{Path, PathBuf};

use crate::{ui::widgets, workspace::Workspace};

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
    let web = ws.git.as_ref().and_then(|git| git.web.clone());
    let folder = line(IconName::HardDrive, short_path(&dir))
        .id("environment-path")
        .cursor_pointer()
        .hover(|row| row.bg(theme.accent))
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.path_menu = !ws.path_menu;
            cx.notify();
        }));
    let menu = ws.path_menu.then(|| path_menu(dir.clone(), web, cx));
    div()
        .flex()
        .flex_col()
        .child(line(IconName::Folder, name))
        .child(
            div()
                .relative()
                .child(folder)
                .children(menu.map(crate::ui::widgets::on_top)),
        )
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
                "{} mode · {} approvals",
                ws.status.mode,
                ws.status.approvals.name()
            )
            .to_lowercase(),
        ))
}

fn path_menu(dir: PathBuf, web: Option<String>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let item = |id: &'static str, icon: IconName, label: &'static str, cx: &App| {
        widgets::menu_item(id, cx)
            .child(widgets::icon(icon, muted))
            .child(label)
    };
    let (reveal, terminal, copy) = (dir.clone(), dir.clone(), dir);
    let mut menu = div()
        .id("path-menu")
        .absolute()
        .top(px(30.))
        .left(px(12.))
        .w(px(220.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .occlude()
        .child(
            item("path-reveal", IconName::Folder, reveal_label(), cx).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.path_menu = false;
                    cx.reveal_path(&reveal);
                },
            )),
        );
    if let Some(web) = web {
        menu = menu.child(
            item("path-repo", IconName::Github, "Open repo", cx).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.path_menu = false;
                    cx.open_url(&web);
                },
            )),
        );
    }
    menu.child(
        item(
            "path-terminal",
            IconName::SquareTerminal,
            "Open in terminal",
            cx,
        )
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.path_menu = false;
            if let Err(error) = open_terminal(&terminal) {
                ws.fail(error, cx);
            }
        })),
    )
    .child(
        item("path-copy", IconName::Copy, "Copy path", cx).on_click(cx.listener(
            move |ws, _, _, cx| {
                ws.path_menu = false;
                cx.write_to_clipboard(ClipboardItem::new_string(copy.display().to_string()));
            },
        )),
    )
    .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
        ws.path_menu = false;
        cx.notify();
    }))
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in files"
    }
}

/// The system terminal in `dir`, started without a shell between.
fn open_terminal(dir: &Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let mut command = if cfg!(target_os = "macos") {
        let mut open = std::process::Command::new("open");
        open.args(["-a", "Terminal"]).arg(dir);
        open
    } else {
        let mut terminal = std::process::Command::new("x-terminal-emulator");
        terminal.current_dir(dir);
        terminal
    };
    let mut child = command.spawn().context("cannot open a terminal")?;
    // Reaped off the window's thread, so no zombie is left.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// `~/code/app` for a folder under home.
fn short_path(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if shown.starts_with(&home) => format!("~{}", &shown[home.len()..]),
        _ => shown,
    }
}
