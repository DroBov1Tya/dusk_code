//! The project's repository: its branch, the changes, the branches to switch to and
//! the worktrees. The core refuses a switch while changes would be lost.

use crate::ui::Type;
use crate::ui::press::Press;
use dusk_protocol::project::ChangeKind;
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::pulls;
use crate::{
    ui::{icons, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(git) = &ws.git else {
        return div().child(widgets::muted("Pick a project to see its repository.", cx));
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let Some(branch) = git
        .branch
        .clone()
        .or_else(|| (!git.branches.is_empty()).then(|| "detached".to_owned()))
    else {
        return div().child(widgets::muted("Not a git repository.", cx));
    };
    let sync = git
        .ahead_behind
        .map(|(ahead, behind)| format!("↑{ahead} ↓{behind}"))
        .unwrap_or_default();
    let mut column = div().flex().flex_col().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .pt_3()
            .child(widgets::icon(
                Icon::default().path(icons::GIT_BRANCH),
                theme.primary,
            ))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(branch.clone()),
            )
            .child(div().meta().text_color(muted).child(sync))
            .child(div().flex_1())
            .child(
                Button::new("git-open-review")
                    .ghost()
                    .small()
                    .icon(Icon::default().path(icons::GIT_COMPARE))
                    .tooltip("Commit, branch, pull and push")
                    .on_click(cx.listener(|ws, _, _, cx| ws.open_review(cx))),
            )
            .child(
                Button::new("git-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip("Read the repository again")
                    .on_click(cx.listener(|ws, _, _, cx| ws.load_git(cx))),
            ),
    );
    let stat = git.stat;
    let repository = ws
        .repository
        .as_ref()
        .map(|status| status.repository.clone())
        .filter(|name| !name.is_empty());
    column = if git.changes.is_empty() {
        column.child(widgets::muted(
            "Clean: nothing differs from the last commit.",
            cx,
        ))
    } else {
        let (ok, bad) = (cx.theme().success, cx.theme().danger);
        column.child(
            div()
                .id("review-open")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .mx_2()
                .px_2()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|row| row.bg(cx.theme().list_hover))
                .press(cx.theme().list_hover, cx)
                .children(repository.map(|name| div().small().truncate().child(name)))
                .child(
                    div()
                        .small()
                        .text_color(ok)
                        .child(format!("+{}", pulls::grouped(stat.added))),
                )
                .child(
                    div()
                        .small()
                        .text_color(bad)
                        .child(format!("\u{2212}{}", pulls::grouped(stat.removed))),
                )
                .child(div().flex_1())
                .child(div().meta().text_color(muted).child(widgets::count(
                    stat.files.max(git.changes.len() as u64) as usize,
                    "file",
                )))
                .child(widgets::icon(IconName::ChevronRight, muted))
                .on_click(cx.listener(|ws, _, _, cx| ws.open_review(cx))),
        )
    };
    column = column.child(pulls::rows(ws, cx));
    column = column.child(widgets::heading("Branches", cx));
    for (index, name) in git.branches.iter().enumerate() {
        let current = *name == branch;
        let target = name.clone();
        column = column.child(
            widgets::row(("branch", index), current, cx)
                .child(widgets::icon(
                    Icon::default().path(icons::GIT_BRANCH),
                    muted,
                ))
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
                    .meta()
                    .flex()
                    .flex_col()
                    .child(tree.branch.clone().unwrap_or_else(|| "detached".to_owned()))
                    .child(
                        div()
                            .text_color(muted)
                            .truncate()
                            .child(tree.path.display().to_string()),
                    ),
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
        self.load_repository(cx);
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
