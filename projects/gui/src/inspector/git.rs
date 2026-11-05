//! The project's repository: its branch, the changes, the branches to switch to and
//! the worktrees. The core refuses a switch while changes would be lost.

use dusk_protocol::project::ChangeKind;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(git) = &ws.git else {
        return div().child(widgets::muted("Pick a project to see its repository.", cx));
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let Some(branch) = git.branch.clone().or_else(|| (!git.branches.is_empty()).then(|| "detached".to_owned())) else {
        return div().child(widgets::muted("Not a git repository.", cx));
    };
    let sync = git
        .ahead_behind
        .map(|(ahead, behind)| format!("↑{ahead} ↓{behind}"))
        .unwrap_or_default();
    let mut column = div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .pt_3()
                .child(widgets::icon(IconName::Github, theme.primary))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(branch.clone()))
                .child(div().text_xs().text_color(muted).child(sync))
                .child(div().flex_1())
                .child(
                    Button::new("git-refresh")
                        .ghost()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .on_click(cx.listener(|ws, _, _, cx| ws.load_git(cx))),
                ),
        )
        .child(widgets::heading(format!("Changes ({})", git.changes.len()), cx));
    for change in git.changes.iter().take(200) {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .px_3()
                .text_xs()
                .child(
                    div()
                        .w(px(12.))
                        .text_color(change_color(change.kind, cx))
                        .child(change_letter(change.kind)),
                )
                .child(div().truncate().child(change.path.clone())),
        );
    }
    column = column.child(widgets::heading("Branches", cx));
    for (index, name) in git.branches.iter().enumerate() {
        let current = *name == branch;
        let target = name.clone();
        column = column.child(
            widgets::row(("branch", index), current, cx)
                .child(widgets::icon(IconName::Github, muted))
                .child(div().truncate().child(name.clone()))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if !current {
                        ws.switch_branch(target.clone(), cx);
                    }
                })),
        );
    }
    if git.worktrees.len() > 1 {
        column = column.child(widgets::heading("Worktrees", cx));
        for tree in &git.worktrees {
            column = column.child(
                div()
                    .px_3()
                    .py_0p5()
                    .text_xs()
                    .flex()
                    .flex_col()
                    .child(tree.branch.clone().unwrap_or_else(|| "detached".to_owned()))
                    .child(div().text_color(muted).truncate().child(tree.path.display().to_string())),
            );
        }
    }
    column
}

pub fn change_letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
        ChangeKind::Untracked => "U",
        ChangeKind::Conflicted => "!",
    }
}

pub fn change_color(kind: ChangeKind, cx: &App) -> Hsla {
    let theme = cx.theme();
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => theme.success,
        ChangeKind::Modified | ChangeKind::Renamed => theme.warning,
        ChangeKind::Deleted | ChangeKind::Conflicted => theme.danger,
    }
}

impl Workspace {
    pub fn load_git(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.projects().git(&id).await },
            |ws, result, cx| match result {
                Ok(git) => {
                    ws.git = Some(git);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn switch_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.projects().switch_branch(&id, &branch).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
                ws.files.clear();
                ws.load_git(cx);
            },
        );
    }
}
