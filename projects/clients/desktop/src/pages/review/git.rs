//! The repository's actions on the review page: a new branch, fetch, pull and push, the
//! commit box, and staging or discarding the chosen file. The core checks and runs each;
//! push and discard are asked of the user first.

use dusk_protocol::client::GitRequest;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use crate::{modal::Modal, workspace::Workspace};

/// What the user is asked before a git request that cannot be taken back.
#[derive(Clone)]
pub enum GitConfirm {
    Push,
    Discard { path: String },
}

pub struct GitBox {
    pub message: Entity<InputState>,
    pub branch: Entity<InputState>,
    /// A request is running; the buttons wait for it.
    pub busy: bool,
}

impl GitBox {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            message: cx.new(|cx| InputState::new(window, cx).placeholder("Commit message")),
            branch: cx.new(|cx| InputState::new(window, cx).placeholder("new-branch")),
            busy: false,
        }
    }
}

/// The two rows under the review's header.
pub fn bar(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let busy = ws.review.git.busy;
    let staged = ws
        .git
        .as_ref()
        .map_or(0, |git| git.changes.iter().filter(|c| c.staged).count());
    let branch_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(div().w(px(220.)).child(Input::new(&ws.review.git.branch)))
        .child(
            Button::new("git-new-branch")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .label("New branch")
                .tooltip("Make a branch from the current commit")
                .on_click(cx.listener(move |ws, _, window, cx| {
                    let name = ws.review.git.branch.read(cx).value().trim().to_owned();
                    if busy || name.is_empty() {
                        return;
                    }
                    ws.review
                        .git
                        .branch
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    ws.run_git(GitRequest::CreateBranch { name, base: None }, cx);
                })),
        )
        .child(div().flex_1())
        .child(action(
            "git-fetch",
            "Fetch",
            IconName::RefreshCw,
            busy,
            cx,
            |ws, cx| ws.run_git(GitRequest::Fetch, cx),
        ))
        .child(action(
            "git-pull",
            "Pull",
            IconName::ArrowDown,
            busy,
            cx,
            |ws, cx| ws.run_git(GitRequest::Pull, cx),
        ))
        .child(action(
            "git-push",
            "Push",
            IconName::ArrowUp,
            busy,
            cx,
            |ws, cx| {
                ws.modal = Some(Modal::Git(GitConfirm::Push));
                cx.notify();
            },
        ));
    let commit_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(Input::new(&ws.review.git.message)))
        .child(action(
            "git-stage-all",
            "Stage all",
            IconName::Plus,
            busy,
            cx,
            |ws, cx| ws.run_git(GitRequest::StageAll, cx),
        ))
        .child(
            Button::new("git-commit")
                .primary()
                .small()
                .label(format!("Commit {staged} staged"))
                .on_click(cx.listener(move |ws, _, window, cx| {
                    let message = ws.review.git.message.read(cx).value().trim().to_owned();
                    if busy || message.is_empty() {
                        ws.toast("Write a commit message first".into(), cx);
                        return;
                    }
                    ws.review
                        .git
                        .message
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    ws.run_git(GitRequest::Commit { message }, cx);
                })),
        );
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(border)
        .child(branch_row)
        .child(commit_row)
}

fn action(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    busy: bool,
    cx: &mut Context<Workspace>,
    run: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .label(label)
        .on_click(cx.listener(move |ws, _, _, cx| {
            if !busy {
                run(ws, cx);
            }
        }))
}

/// The yes-or-no card of a push or a discard.
pub fn confirm(confirm: &GitConfirm, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (question, detail, yes) = match confirm {
        GitConfirm::Push => (
            "Push the current branch?".to_owned(),
            "Its commits go to the remote, to its upstream or to origin as a new one. \
             A push is never forced: the remote may refuse it."
                .to_owned(),
            "Push",
        ),
        GitConfirm::Discard { path } => (
            format!("Discard the changes of {path}?"),
            "Its uncommitted changes are lost; a new file is deleted. This cannot be undone."
                .to_owned(),
            "Discard",
        ),
    };
    let request = match confirm {
        GitConfirm::Push => GitRequest::Push,
        GitConfirm::Discard { path } => GitRequest::Discard { path: path.clone() },
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(question)
        .child(div().text_sm().text_color(muted).child(detail))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("git-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("git-confirm")
                        .danger()
                        .label(yes)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.modal = None;
                            ws.run_git(request.clone(), cx);
                        })),
                ),
        )
}

impl Workspace {
    /// Sends one git request for the chosen project and shows what git said.
    pub fn run_git(&mut self, request: GitRequest, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        self.review.git.busy = true;
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.projects().repository(&id, request).await },
            |ws, result, cx| {
                ws.review.git.busy = false;
                match result {
                    Ok(outcome) if !outcome.conflicts.is_empty() => ws.fail(
                        anyhow::anyhow!("conflicts in {}", outcome.conflicts.join(", ")),
                        cx,
                    ),
                    Ok(outcome) => {
                        let said = outcome.message.lines().last().unwrap_or("Done").to_owned();
                        ws.toast(if said.is_empty() { "Done".into() } else { said }, cx);
                    }
                    Err(error) => ws.fail(error, cx),
                }
            },
        );
    }

    /// The core changed a project's repository: the shown one is read again.
    pub fn git_changed(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.project.as_deref() != Some(project_id) {
            return;
        }
        self.load_git(cx);
        if self.center == crate::workspace::Center::Review {
            self.open_review(cx);
        }
    }
}
