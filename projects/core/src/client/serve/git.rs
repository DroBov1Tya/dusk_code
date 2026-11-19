//! Requests that change a repository or read its pull requests. They may wait on the
//! network or a credential helper, so they are answered beside the loop; a change is
//! told to the clients that may read projects.

use std::sync::Arc;

use anyhow::Result;
use dusk_protocol::client::{GitRequest, Response};
use tokio::sync::oneshot;

use super::{Server, publish};
use crate::{
    project::{self, forge, git},
    runtime::Runtime,
};

impl Server {
    pub(super) fn repository(
        &self,
        id: String,
        request: GitRequest,
        view: super::ErrorView,
        reply: oneshot::Sender<Response>,
    ) {
        let runtime = Arc::clone(&self.runtime);
        let subscribers = Arc::clone(&self.subscribers);
        let tracker = self.pulls.clone();
        tokio::spawn(async move {
            let changes = changes_repository(&request);
            let response = match answer(&runtime, &id, request).await {
                Ok(response) => response,
                Err(error) => Response::Error(view.text(&error)),
            };
            // A failed merge or pull can still leave the repository changed.
            if changes {
                publish::git_changed(&subscribers, &id);
                tracker.changed(&id);
            }
            let _ = reply.send(response);
        });
    }
}

fn changes_repository(request: &GitRequest) -> bool {
    !matches!(
        request,
        GitRequest::PullRequests | GitRequest::PullRequest { .. }
    )
}

async fn answer(runtime: &Runtime, id: &str, request: GitRequest) -> Result<Response> {
    let project = project::find(runtime.state(), id).await?;
    let project = &project;
    Ok(Response::GitDone(match request {
        GitRequest::CreateBranch { name, base, switch } => {
            git::create_branch(project, &name, base.as_deref(), switch).await?
        }
        GitRequest::Stage { paths } => git::stage(project, &paths).await?,
        GitRequest::StageAll => git::stage_all(project).await?,
        GitRequest::Unstage { paths } => git::unstage(project, &paths).await?,
        GitRequest::Commit { message } => git::commit(project, &message).await?,
        GitRequest::Discard { path } => git::discard(project, &path).await?,
        GitRequest::Merge { branch } => git::merge(project, &branch).await?,
        GitRequest::AbortMerge => git::abort_merge(project).await?,
        GitRequest::AddWorktree { branch, base } => {
            git::add_worktree(project, &branch, base.as_deref()).await?
        }
        GitRequest::RemoveWorktree { path } => git::remove_worktree(project, &path).await?,
        GitRequest::Fetch => git::fetch(project).await?,
        GitRequest::Pull => git::pull(project).await?,
        GitRequest::Push => git::push(project).await?,
        GitRequest::PullRequests => return Ok(Response::PullRequests(forge::list(project).await?)),
        GitRequest::PullRequest { number } => {
            return Ok(Response::PullRequest(Box::new(
                forge::view(project, number).await?,
            )));
        }
        GitRequest::CreatePullRequest { title, body, base } => {
            return Ok(Response::PullRequest(Box::new(
                forge::create(project, &title, &body, base.as_deref()).await?,
            )));
        }
    }))
}
