//! The repository status of projects: answered beside the loop, since a forge may take
//! a while, and read again when the tracker says a project is due. A change is told to
//! the clients that may read projects.

use std::sync::Arc;

use anyhow::Result;
use dusk_protocol::{
    client::Response,
    project::{PullStatus, RepositoryStatus},
};
use tokio::sync::oneshot;

use super::{ClientId, Server, Subscribers, publish, thread_pulls};
use crate::{
    project::{self, forge},
    runtime::Runtime,
};

impl Server {
    /// The asking client watches the project from now on; a status the core has is
    /// answered at once.
    pub(super) fn repository_status(
        &self,
        client: ClientId,
        id: String,
        view: super::ErrorView,
        reply: oneshot::Sender<Response>,
    ) {
        if let Some(status) = self.pulls.watch(client, &id) {
            let _ = reply.send(Response::RepositoryStatus(Box::new(status)));
            return;
        }
        self.pulls.begin(&id);
        let (runtime, tracker) = (Arc::clone(&self.runtime), self.pulls.clone());
        let subscribers = Arc::clone(&self.subscribers);
        tokio::spawn(async move {
            let read = refresh(&runtime, &tracker, &subscribers, &id).await;
            let response = match &read {
                Ok(status) => Response::RepositoryStatus(Box::new(status.clone())),
                Err(error) => Response::Error(view.text(error)),
            };
            let _ = reply.send(response);
            if read.is_ok() {
                thread_pulls::sync(&runtime, &tracker, &subscribers, &id).await;
            }
        });
    }

    /// One pull request of the project's own repository, for a card in a chat: from
    /// the tracker when it has it, else read beside the loop, once at a time. Only once
    /// a forge named the prefix of its pulls: until then none, and the project is read,
    /// so the client hears of the status and asks again.
    pub(super) fn pull_status(&self, id: String, number: u64, reply: oneshot::Sender<Response>) {
        let Some(prefix) = self.pulls.prefix(&id) else {
            self.pulls.changed(&id);
            let _ = reply.send(Response::PullStatus(None));
            return;
        };
        match self.pulls.look_up(&id, number) {
            forge::Look::Known(pull) => {
                let _ = reply.send(Response::PullStatus(pull));
            }
            look => {
                let (runtime, tracker) = (Arc::clone(&self.runtime), self.pulls.clone());
                tokio::spawn(async move {
                    let pull = fetch(look, &runtime, &tracker, &id, number, &prefix).await;
                    let _ = reply.send(Response::PullStatus(pull.map(Box::new)));
                });
            }
        }
    }

    /// A pull request from the project's current branch, answered beside the loop since
    /// the forge may take a while. The repository is told changed either way: a failed
    /// call can still have made the pull request. One made from a chat is kept as one of
    /// its pull requests.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_pull(
        &self,
        id: String,
        title: String,
        body: String,
        base: Option<String>,
        thread_id: Option<String>,
        view: super::ErrorView,
        reply: oneshot::Sender<Response>,
    ) {
        let runtime = Arc::clone(&self.runtime);
        let subscribers = Arc::clone(&self.subscribers);
        let tracker = self.pulls.clone();
        tokio::spawn(async move {
            let made = async {
                let project = project::find(runtime.state(), &id).await?;
                let forges = forges(&runtime, &id).await?;
                let pull = forge::create(&project, &title, &body, base.as_deref(), &forges).await?;
                anyhow::Ok((project, pull))
            };
            let response = match made.await {
                Ok((project, pull)) => {
                    if let Some(thread_id) = thread_id {
                        keep_made(&runtime, &subscribers, &project.dir, thread_id, &pull).await;
                    }
                    Response::PullStatus(Some(Box::new(pull)))
                }
                Err(error) => Response::Error(view.text(&error)),
            };
            publish::git_changed(&subscribers, &id);
            tracker.changed(&id);
            let _ = reply.send(response);
        });
    }

    /// Starts a read of every project the tracker says is due.
    pub(super) fn track_repositories(&self) {
        for id in self.pulls.due() {
            let (runtime, tracker) = (Arc::clone(&self.runtime), self.pulls.clone());
            let subscribers = Arc::clone(&self.subscribers);
            tokio::spawn(async move {
                match refresh(&runtime, &tracker, &subscribers, &id).await {
                    Ok(_) => thread_pulls::sync(&runtime, &tracker, &subscribers, &id).await,
                    Err(error) => tracing::debug!("repository status of {id} not read: {error:#}"),
                }
            });
        }
    }

    pub(super) async fn pull_summaries(&self) -> Result<Response> {
        let projects = self.runtime.state().list_projects().await?;
        let ids: Vec<String> = projects.into_iter().map(|project| project.id).collect();
        Ok(Response::PullSummaries(self.pulls.summaries(&ids)))
    }
}

/// Pull request `number` of project `id` as `look` says to get it: at hand, from the
/// read under way, or read now and handed to the tracker.
pub(super) async fn fetch(
    look: forge::Look,
    runtime: &Runtime,
    tracker: &forge::Tracker,
    id: &str,
    number: u64,
    prefix: &str,
) -> Option<PullStatus> {
    match look {
        forge::Look::Known(pull) => pull.map(|pull| *pull),
        forge::Look::Wait(waiting) => waiting.await.ok().flatten(),
        forge::Look::Read => {
            let pull = match project::find(runtime.state(), id).await {
                Ok(project) => match forges(runtime, id).await {
                    Ok(forges) => forge::read_pull(&project, number, &forges, prefix).await,
                    Err(_) => None,
                },
                Err(_) => None,
            };
            tracker.looked_up(id, number, pull.clone());
            pull
        }
    }
}

/// Keeps a pull request made from chat `thread_id` as one of its own; the repository is
/// the one the remote names, as a forge's status names it.
async fn keep_made(
    runtime: &Runtime,
    subscribers: &Subscribers,
    dir: &std::path::Path,
    thread_id: String,
    pull: &PullStatus,
) {
    let Some(remote) = forge::Remote::of(dir).await else {
        return;
    };
    let state = runtime.state_of(&thread_id).await;
    let kept = async {
        if state
            .add_thread_pull(&thread_id, &remote.path, pull)
            .await?
        {
            let pulls = state.thread_pulls(&thread_id).await?;
            thread_pulls::tell(subscribers, thread_id.clone(), pulls);
        }
        anyhow::Ok(())
    };
    if let Err(error) = kept.await {
        tracing::warn!(
            "pull request {} not kept for {thread_id}: {error:#}",
            pull.number
        );
    }
}

/// Reads the project's status, keeps it, and tells the clients when it changed.
async fn refresh(
    runtime: &Runtime,
    tracker: &forge::Tracker,
    subscribers: &Subscribers,
    id: &str,
) -> Result<RepositoryStatus> {
    let project = match project::find(runtime.state(), id).await {
        Ok(project) => project,
        Err(error) => {
            tracker.failed(id);
            return Err(error);
        }
    };
    let forges = match forges(runtime, id).await {
        Ok(forges) => forges,
        Err(error) => {
            tracker.failed(id);
            return Err(error);
        }
    };
    let (status, changed) = tracker.finish(id, forge::read_status(&project, &forges).await);
    if changed {
        publish::repository_status(subscribers, id, &status);
    }
    Ok(status)
}

/// What a read of project `id` asks a forge plugin with: the switch and the choices of
/// now, and every other saved project's folder to hide from the call.
pub(super) async fn forges(runtime: &Runtime, id: &str) -> Result<forge::Forges> {
    let config = runtime.config().current();
    let on = config.contracts_on();
    // Only a plugin's call hides them; with the switch off none is made.
    let others = if on {
        runtime
            .state()
            .list_projects()
            .await?
            .into_iter()
            .filter(|project| project.id != id)
            .map(|project| project.dir)
            .collect()
    } else {
        Vec::new()
    };
    Ok(forge::Forges {
        contracts: runtime.contracts(),
        on,
        chosen: config.contracts().forge.clone(),
        others,
    })
}
