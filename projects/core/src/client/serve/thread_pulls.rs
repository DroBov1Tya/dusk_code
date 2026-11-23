//! The pull requests of each chat, kept up to date beside the loop. A project's chats are
//! looked through for new links when its repository status was read and when one of them
//! got a message; each kept pull request takes what that status says of it, or is read
//! once from the forge until it is merged or closed. No reads of their own: they ride on
//! the tracker's, and a link before the forge named its pulls waits for the next one.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use dusk_protocol::{client::Event, project::PullStatus, thread::ThreadEvent};
use parking_lot::Mutex;
use tracing::debug;

use super::{Subscribers, publish::publish, pulls::fetch};
use crate::{
    project::{self, forge},
    runtime::Runtime,
    state::slim_pull as slim,
};

/// Reads of pull requests no status lists, at most, each time the chats are brought up
/// to date; the rest, tried least lately first, wait for the next time.
const MAX_READS: usize = 8;

/// The projects whose chats are being brought up to date, each with whether it was asked
/// again meanwhile: one pass at a time per project, and one more after it when asked.
#[derive(Clone, Default)]
pub(super) struct Syncs(Arc<Mutex<HashMap<String, bool>>>);

impl Syncs {
    /// True when the caller is to make the pass; else the one under way makes another.
    fn begin(&self, id: &str) -> bool {
        let mut passes = self.0.lock();
        match passes.get_mut(id) {
            Some(again) => {
                *again = true;
                false
            }
            None => {
                passes.insert(id.to_owned(), false);
                true
            }
        }
    }

    /// True when another pass was asked for while this one ran.
    fn again(&self, id: &str) -> bool {
        let mut passes = self.0.lock();
        if passes.get(id) == Some(&true) {
            passes.insert(id.to_owned(), false);
            return true;
        }
        passes.remove(id);
        false
    }
}

/// Brings the pull requests of project `id`'s chats up to date and tells the clients of
/// each chat whose list changed. A failure is logged: the next status tries again.
pub(super) async fn sync(
    runtime: &Runtime,
    tracker: &forge::Tracker,
    subscribers: &Subscribers,
    syncs: &Syncs,
    id: &str,
) {
    if !syncs.begin(id) {
        return;
    }
    loop {
        if let Err(error) = try_sync(runtime, tracker, subscribers, id).await {
            debug!("pull requests of the chats of {id} not brought up to date: {error:#}");
        }
        if !syncs.again(id) {
            break;
        }
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
    // What each number came to in this pass, and whether the forge answered it has none.
    let mut read: HashMap<u64, (Option<PullStatus>, bool)> = HashMap::new();
    let mut reads = 0;
    for state in runtime.states() {
        let linked = state.find_pull_links(&dir, repository, prefix).await?;
        let mut changed: HashSet<String> = linked.into_iter().collect();
        for kept in state.folder_pulls(&dir, repository).await? {
            let listed = served.pulls.iter().find(|pull| pull.number == kept.number);
            let now = match listed {
                Some(pull) => Some(pull.clone()),
                None if kept.settled() => None,
                None => {
                    let (pull, missed) = match read.get(&kept.number) {
                        Some(answer) => answer.clone(),
                        // A look-up may start a read, which must then be made: it is
                        // not begun once the reads of this pass are spent.
                        None if reads >= MAX_READS => continue,
                        None => {
                            let look = tracker.look_up(id, kept.number);
                            let forge_read = matches!(look, forge::Look::Read);
                            reads += usize::from(forge_read);
                            let fetched = fetch(look, runtime, tracker, id, kept.number, prefix);
                            // Only the forge saying there is none is a miss: a read that
                            // failed (signed out, rate limited, offline) says nothing.
                            let answer = match fetched.await {
                                Ok(pull) => {
                                    let missed = forge_read && pull.is_none();
                                    (pull, missed)
                                }
                                Err(error) => {
                                    debug!("{error:#}");
                                    (None, false)
                                }
                            };
                            read.insert(kept.number, answer.clone());
                            answer
                        }
                    };
                    state
                        .tried_thread_pull(&kept.thread_id, repository, kept.number, missed)
                        .await?;
                    pull
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
    syncs: &Syncs,
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
    sync(runtime, tracker, subscribers, syncs, &project.id).await;
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
