//! The pull requests of each chat, kept up to date beside the loop. A project's chats are
//! looked through for new links when its repository status was read and when one of them
//! got a message; each kept pull request takes what that status says of it, or is read
//! once from the forge until it is merged or closed. No reads of their own: they ride on
//! the tracker's, and a link before the forge named its pulls waits for the next one.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use dusk_protocol::{client::Event, project::PullStatus, thread::ThreadEvent};
use tracing::debug;

use super::{Subscribers, publish::publish, pulls::fetch};
use crate::{
    project::{self, forge},
    runtime::Runtime,
    state::slim_pull as slim,
};

/// Reads of pull requests no status lists, at most, each time the chats are brought up
/// to date; the rest wait for the next time.
const MAX_READS: usize = 8;

/// Brings the pull requests of project `id`'s chats up to date and tells the clients of
/// each chat whose list changed. A failure is logged: the next status tries again.
pub(super) async fn sync(
    runtime: &Runtime,
    tracker: &forge::Tracker,
    subscribers: &Subscribers,
    id: &str,
) {
    if let Err(error) = try_sync(runtime, tracker, subscribers, id).await {
        debug!("pull requests of the chats of {id} not brought up to date: {error:#}");
    }
}

async fn try_sync(
    runtime: &Runtime,
    tracker: &forge::Tracker,
    subscribers: &Subscribers,
    id: &str,
) -> Result<()> {
    let Some(served) = tracker.served(id) else {
        return Ok(());
    };
    let project = project::find(runtime.state(), id).await?;
    let dir = project.dir.to_string_lossy().into_owned();
    let (repository, prefix) = (&served.repository, &served.pull_prefix);
    let mut read: HashMap<u64, Option<PullStatus>> = HashMap::new();
    for state in runtime.states() {
        let linked = state.find_pull_links(&dir, repository, prefix).await?;
        let mut changed: HashSet<String> = linked.into_iter().collect();
        for kept in state.folder_pulls(&dir, repository).await? {
            let listed = served.pulls.iter().find(|pull| pull.number == kept.number);
            let now = match listed {
                Some(pull) => Some(pull.clone()),
                None if kept.settled() => None,
                None => {
                    if !read.contains_key(&kept.number) {
                        if read.len() >= MAX_READS {
                            continue;
                        }
                        let look = tracker.look_up(id, kept.number);
                        let pull = fetch(look, runtime, tracker, id, kept.number, prefix).await;
                        read.insert(kept.number, pull);
                    }
                    read.get(&kept.number).cloned().flatten()
                }
            };
            if let Some(pull) = now.map(slim)
                && kept.pull.as_ref() != Some(&pull)
            {
                state
                    .set_thread_pull(&kept.thread_id, repository, &pull)
                    .await?;
                changed.insert(kept.thread_id);
            }
        }
        for thread_id in changed {
            let pulls = state.thread_pulls(&thread_id).await?;
            tell(subscribers, thread_id, pulls);
        }
    }
    Ok(())
}

/// A chat got a message: its project's chats are looked through when the forge has named
/// the prefix of its pulls, else its status is read first where a client watches it.
pub(super) async fn chat_spoke(
    runtime: &Runtime,
    tracker: &forge::Tracker,
    subscribers: &Subscribers,
    thread_id: &str,
) {
    let state = runtime.state_of(thread_id).await;
    let Ok(Some(thread)) = state.load_thread(thread_id).await else {
        return;
    };
    let Some(dir) = thread.project.filter(|_| thread.parent_thread_id.is_none()) else {
        return;
    };
    let projects = match runtime.state().list_projects().await {
        Ok(projects) => projects,
        Err(error) => {
            debug!("projects not read for the pulls of {thread_id}: {error:#}");
            return;
        }
    };
    let Some(project) = projects
        .into_iter()
        .find(|project| project.dir.to_string_lossy() == dir)
    else {
        return;
    };
    if tracker.served(&project.id).is_none() {
        tracker.changed(&project.id);
        return;
    }
    sync(runtime, tracker, subscribers, &project.id).await;
}

/// Tells every client listing the chat its pull requests now.
pub(super) fn tell(
    subscribers: &Subscribers,
    thread_id: String,
    pulls: Vec<dusk_protocol::project::ThreadPull>,
) {
    let event = Event::Thread {
        thread_id,
        event: Box::new(ThreadEvent::Pulls(pulls)),
    };
    publish(subscribers, event);
}
