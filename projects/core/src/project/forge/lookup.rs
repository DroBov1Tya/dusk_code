//! Single pull requests a chat links, read for a card. Each answer is kept a few
//! minutes, a pull request is read once at a time with the askers in between waiting
//! for that read, few are kept, and few reads run at once, so a chat full of links
//! cannot make the core ask a forge without end.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use dusk_protocol::{
    plugin::contract::forge::PullArgs,
    project::{Project, PullStatus},
};
use tokio::sync::{Semaphore, oneshot};
use tracing::debug;

use super::{Forges, Remote, checked, status};

/// How long an answer, a missing pull request too, is given again without a read.
const FRESH: Duration = Duration::from_secs(180);
/// Pull requests kept; the oldest answered one makes room.
const MAX_KEPT: usize = 64;
/// Askers waiting on one read; more are answered with nothing.
const MAX_WAITING: usize = 16;
/// Reads of single pull requests at once, over every project.
static READS: Semaphore = Semaphore::const_new(4);

/// What to do for one asker.
pub enum Look {
    /// The answer is at hand: the pull request, or that there is none to show.
    Known(Option<Box<PullStatus>>),
    /// A read is under way; its answer comes here.
    Wait(oneshot::Receiver<Option<PullStatus>>),
    /// Nobody reads it: the asker does, and hands the answer to `done`.
    Read,
}

#[derive(Default)]
pub struct Lookups(HashMap<(String, u64), Entry>);

struct Entry {
    pull: Option<PullStatus>,
    at: Instant,
    /// `Some` while a read is under way: who waits for it.
    waiting: Option<Vec<oneshot::Sender<Option<PullStatus>>>>,
}

impl Lookups {
    pub fn look(&mut self, project_id: &str, number: u64, now: Instant) -> Look {
        let key = (project_id.to_owned(), number);
        match self.0.get_mut(&key) {
            Some(Entry {
                waiting: Some(waiting),
                ..
            }) => {
                if waiting.len() >= MAX_WAITING {
                    return Look::Known(None);
                }
                let (sender, receiver) = oneshot::channel();
                waiting.push(sender);
                return Look::Wait(receiver);
            }
            Some(entry) if now.duration_since(entry.at) < FRESH => {
                return Look::Known(entry.pull.clone().map(Box::new));
            }
            Some(_) => {}
            None => {
                if self.0.len() >= MAX_KEPT && !self.evict() {
                    return Look::Known(None);
                }
            }
        }
        let entry = Entry {
            pull: None,
            at: now,
            waiting: Some(Vec::new()),
        };
        self.0.insert(key, entry);
        Look::Read
    }

    /// The read of `look`'s `Read` ended; its waiters hear the answer.
    pub fn done(&mut self, project_id: &str, number: u64, pull: Option<PullStatus>, now: Instant) {
        let Some(entry) = self.0.get_mut(&(project_id.to_owned(), number)) else {
            return;
        };
        for waiter in entry.waiting.take().into_iter().flatten() {
            let _ = waiter.send(pull.clone());
        }
        entry.pull = pull;
        entry.at = now;
    }

    /// Drops the oldest entry no read is under way for; false when every one has one.
    fn evict(&mut self) -> bool {
        let oldest = self
            .0
            .iter()
            .filter(|(_, entry)| entry.waiting.is_none())
            .min_by_key(|(_, entry)| entry.at)
            .map(|(key, _)| key.clone());
        oldest.is_some_and(|key| self.0.remove(&key).is_some())
    }
}

/// Reads pull request `number` of the project's own repository, waiting its turn; only
/// a pull whose page is `prefix`, the one its last status named, and its number.
pub async fn read(
    project: &Project,
    number: u64,
    forges: &Forges,
    prefix: &str,
) -> Option<PullStatus> {
    let _turn = READS.acquire().await.ok()?;
    let remote = Remote::of(&project.dir).await?;
    let Some(served) = forges.route(&remote.host) else {
        return status::read_one(project, number, prefix).await;
    };
    let args = PullArgs {
        host: remote.host.clone(),
        repository: remote.path.clone(),
        number,
    };
    let answer = forges.call(&served, "pull", &project.dir, &args).await;
    match answer.and_then(|ok| checked::pull(ok, prefix, &remote.host, Some(number))) {
        Ok(pull) => pull,
        Err(error) => {
            debug!("pull request {number} not read: {}", error.message);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(number: u64) -> PullStatus {
        PullStatus {
            number,
            title: "t".to_owned(),
            state: dusk_protocol::project::PullState::Open,
            branch: "b".to_owned(),
            author: String::new(),
            created: None,
            updated: None,
            added: 0,
            removed: 0,
            files: None,
            url: String::new(),
            checks_url: String::new(),
            checks: Default::default(),
        }
    }

    #[test]
    fn one_read_at_a_time_then_kept_a_while_and_bounded() {
        let mut lookups = Lookups::default();
        let now = Instant::now();
        assert!(matches!(lookups.look("p", 7, now), Look::Read));
        let Look::Wait(mut waiting) = lookups.look("p", 7, now) else {
            panic!("a second asker waits for the read under way");
        };
        lookups.done("p", 7, Some(pull(7)), now);
        assert_eq!(waiting.try_recv().unwrap().unwrap().number, 7);
        assert!(matches!(lookups.look("p", 7, now), Look::Known(Some(_))));
        assert!(matches!(lookups.look("p", 7, now + FRESH), Look::Read));

        // Full of reads under way: nothing is evicted, nothing more is read.
        let mut full = Lookups::default();
        for number in 0..MAX_KEPT as u64 {
            assert!(matches!(full.look("p", number, now), Look::Read));
        }
        assert!(matches!(full.look("p", 1000, now), Look::Known(None)));
        full.done("p", 3, None, now);
        assert!(matches!(full.look("p", 1000, now), Look::Read));
        assert_eq!(full.0.len(), MAX_KEPT);
    }
}
