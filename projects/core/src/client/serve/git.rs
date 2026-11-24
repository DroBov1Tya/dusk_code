//! Requests that change a repository. They may wait on the network or a credential
//! helper, so they are answered beside the loop; a change is
//! told to the clients that may read projects.

use std::sync::Arc;

use anyhow::Result;
use dusk_protocol::client::{GitRequest, Response};
use tokio::sync::oneshot;

use super::{Server, publish};
use crate::{
    project::{self, git},
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
            let response = match answer(&runtime, &id, request).await {
                Ok(response) => response,
                Err(error) => Response::Error(view.text(&error)),
            };
            // A failed merge or pull can still leave the repository changed.
            publish::git_changed(&subscribers, &id);
            tracker.changed(&id);
            let _ = reply.send(response);
        });
    }
}

async fn answer(runtime: &Runtime, id: &str, request: GitRequest) -> Result<Response> {
    let project = project::find(runtime.state(), id).await?;
    let done = change(&project, request).await;
    // A failed merge or pull can still have changed files.
    runtime.refresh_graph(&project.dir);
    Ok(Response::GitDone(done?))
}

async fn change(
    project: &dusk_protocol::project::Project,
    request: GitRequest,
) -> Result<dusk_protocol::project::GitOutcome> {
    Ok(match request {
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
    })
}
