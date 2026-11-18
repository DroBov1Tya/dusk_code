//! The repository status of projects: answered beside the loop, since GitHub may take a
//! while, and read again when the tracker says a project is due. A change is told to
//! the clients that may read projects.

use std::sync::Arc;

use anyhow::Result;
use dusk_protocol::{
    client::Response,
    project::{PullStatus, RepositoryStatus},
};
use tokio::sync::oneshot;

use super::{ClientId, Server, Subscribers, publish};
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
            let response = match refresh(&runtime, &tracker, &subscribers, &id).await {
                Ok(status) => Response::RepositoryStatus(Box::new(status)),
                Err(error) => Response::Error(format!("{error:#}")),
            };
            let _ = reply.send(response);
        });
    }

    /// One pull request of the project's own repository, for a card in a chat: from
    /// the tracker when it has it, else read beside the loop, once at a time.
    pub(super) fn pull_status(&self, id: String, number: u64, reply: oneshot::Sender<Response>) {
        let answer = |pull: Option<PullStatus>| Response::PullStatus(pull.map(Box::new));
        match self.pulls.look_up(&id, number) {
            forge::Look::Known(pull) => {
                let _ = reply.send(Response::PullStatus(pull));
            }
            forge::Look::Wait(waiting) => {
                tokio::spawn(async move {
                    let _ = reply.send(answer(waiting.await.ok().flatten()));
                });
            }
            forge::Look::Read => {
                let (runtime, tracker) = (Arc::clone(&self.runtime), self.pulls.clone());
                tokio::spawn(async move {
                    let pull = match project::find(runtime.state(), &id).await {
                        Ok(project) => forge::read_pull(&project, number).await,
                        Err(_) => None,
                    };
                    tracker.looked_up(&id, number, pull.clone());
                    let _ = reply.send(answer(pull));
                });
            }
        }
    }

    /// Starts a read of every project the tracker says is due.
    pub(super) fn track_repositories(&self) {
        for id in self.pulls.due() {
            let (runtime, tracker) = (Arc::clone(&self.runtime), self.pulls.clone());
            let subscribers = Arc::clone(&self.subscribers);
            tokio::spawn(async move {
                if let Err(error) = refresh(&runtime, &tracker, &subscribers, &id).await {
                    tracing::debug!("repository status of {id} not read: {error:#}");
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
    let (status, changed) = tracker.finish(id, forge::read_status(&project).await);
    if changed {
        publish::repository_status(subscribers, id, &status);
    }
    Ok(status)
}
