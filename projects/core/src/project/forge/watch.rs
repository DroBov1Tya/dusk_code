//! Keeping the repository status of the projects clients show fresh. A project is read
//! again while a client watches it: soon while checks run, seldom once they settle,
//! later after each failure, and at once after the core changed its repository. A
//! project nobody watches is read only when the summaries find it unread or old. One
//! read of a project is in flight at a time.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use dusk_protocol::project::{
    CheckState, Forge, PullState, PullStatus, PullSummary, RepositoryStatus,
};
use parking_lot::Mutex;

use super::{
    lookup::{Look, Lookups},
    status::Reading,
};

/// How often the server asks what is due.
pub const PERIOD: Duration = Duration::from_secs(5);
/// Between reads while a check runs.
const RUNNING: Duration = Duration::from_secs(30);
/// Between reads once every check is final, or there are none.
const SETTLED: Duration = Duration::from_secs(300);
/// A status younger than this answers a request as it is.
const FRESH: Duration = Duration::from_secs(10);
/// The first wait after a failure; it doubles up to `MAX_BACKOFF`.
const BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(600);

/// Who watches what, and what was last read; shared with the reads beside the loop.
#[derive(Clone, Default)]
pub struct Tracker(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    projects: HashMap<String, Tracked>,
    /// Each client watches one project: the one it last asked the status of.
    watchers: HashMap<u64, String>,
    /// Single pull requests read for the cards of links in chats.
    lookups: Lookups,
}

struct Tracked {
    status: Option<RepositoryStatus>,
    /// The first `mine` pull requests are the current branch's.
    mine: usize,
    read_at: Option<Instant>,
    due: Instant,
    reading: bool,
    failures: u32,
}

impl Tracked {
    fn new(now: Instant) -> Self {
        Self {
            status: None,
            mine: 0,
            read_at: None,
            due: now,
            reading: false,
            failures: 0,
        }
    }
}

impl Tracker {
    /// `client` now watches `project_id`; answers with its status when it is fresh. An
    /// older one is answered too, and read again at the next tick.
    pub fn watch(&self, client: u64, project_id: &str) -> Option<RepositoryStatus> {
        let now = Instant::now();
        let mut inner = self.0.lock();
        inner.watchers.insert(client, project_id.to_owned());
        let tracked = inner
            .projects
            .entry(project_id.to_owned())
            .or_insert_with(|| Tracked::new(now));
        let status = tracked.status.clone()?;
        if tracked.read_at.is_none_or(|at| now - at > FRESH) {
            tracked.due = now;
        }
        Some(status)
    }

    /// Pull request `number` of `project_id`: at hand when its status lists it or it
    /// was read lately; otherwise read, by the asker or by the one already reading it.
    pub fn look_up(&self, project_id: &str, number: u64) -> Look {
        let mut inner = self.0.lock();
        let listed = inner
            .projects
            .get(project_id)
            .and_then(|tracked| tracked.status.as_ref())
            .and_then(|status| status.forge.served())
            .and_then(|served| served.pulls.iter().find(|pull| pull.number == number));
        if let Some(pull) = listed {
            return Look::Known(Some(Box::new(pull.clone())));
        }
        inner.lookups.look(project_id, number, Instant::now())
    }

    /// The read `look_up` asked for ended.
    pub fn looked_up(&self, project_id: &str, number: u64, pull: Option<PullStatus>) {
        let mut inner = self.0.lock();
        inner.lookups.done(project_id, number, pull, Instant::now());
    }

    /// The client is gone; what it watched is no longer read for it.
    pub fn forget(&self, client: u64) {
        self.0.lock().watchers.remove(&client);
    }

    /// The core changed the repository: it is read at the next tick.
    pub fn changed(&self, project_id: &str) {
        if let Some(tracked) = self.0.lock().projects.get_mut(project_id) {
            tracked.due = Instant::now();
        }
    }

    /// Marks a read of `project_id` begun, so the tick does not start another.
    pub fn begin(&self, project_id: &str) {
        let now = Instant::now();
        let mut inner = self.0.lock();
        let tracked = inner
            .projects
            .entry(project_id.to_owned())
            .or_insert_with(|| Tracked::new(now));
        tracked.reading = true;
    }

    /// The projects to read now, each marked begun: every due one a client watches,
    /// and at most one more that only the summaries asked for.
    pub fn due(&self) -> Vec<String> {
        let now = Instant::now();
        let mut inner = self.0.lock();
        let Inner {
            projects, watchers, ..
        } = &mut *inner;
        let mut picked = Vec::new();
        let mut unwatched = false;
        for (id, tracked) in projects.iter_mut() {
            if tracked.reading || tracked.due > now {
                continue;
            }
            let watched = watchers.values().any(|watched| watched == id);
            if !watched && (unwatched || !summary_wants(tracked, now)) {
                continue;
            }
            unwatched |= !watched;
            tracked.reading = true;
            picked.push(id.clone());
        }
        picked
    }

    /// Takes a finished read; answers with the status to show, and whether it differs
    /// from the one before. On trouble the pull requests known before are kept.
    pub fn finish(&self, project_id: &str, reading: Reading) -> (RepositoryStatus, bool) {
        let now = Instant::now();
        let mut inner = self.0.lock();
        let tracked = inner
            .projects
            .entry(project_id.to_owned())
            .or_insert_with(|| Tracked::new(now));
        let Reading {
            mut status,
            mut mine,
            trouble,
        } = reading;
        if trouble {
            tracked.failures = tracked.failures.saturating_add(1);
            tracked.due = now + backoff(tracked.failures);
            let known = tracked
                .status
                .as_ref()
                .and_then(|old| old.forge.served())
                .filter(|old| !old.pulls.is_empty());
            if let (Some(old), Forge::Served(served)) = (known, &mut status.forge) {
                served.pulls = old.pulls.clone();
                served.unavailable = None;
                mine = tracked.mine;
            }
        } else {
            tracked.failures = 0;
            tracked.due = now + if running(&status) { RUNNING } else { SETTLED };
        }
        let changed = tracked.status.as_ref() != Some(&status);
        tracked.status = Some(status.clone());
        tracked.mine = mine;
        tracked.read_at = Some(now);
        tracked.reading = false;
        (status, changed)
    }

    /// A read that could not be made, such as of a project no longer saved.
    pub fn failed(&self, project_id: &str) {
        let now = Instant::now();
        if let Some(tracked) = self.0.lock().projects.get_mut(project_id) {
            tracked.failures = tracked.failures.saturating_add(1);
            tracked.due = now + backoff(tracked.failures);
            tracked.reading = false;
        }
    }

    /// The current branch's pull requests of each of `project_ids` that has some. A
    /// project unread, or read long ago, is marked for a read in the background.
    pub fn summaries(&self, project_ids: &[String]) -> Vec<PullSummary> {
        let now = Instant::now();
        let mut inner = self.0.lock();
        let mut summaries = Vec::new();
        for id in project_ids {
            let tracked = inner
                .projects
                .entry(id.clone())
                .or_insert_with(|| Tracked::new(now));
            let Some(status) = &tracked.status else {
                continue;
            };
            let Some(served) = status.forge.served() else {
                continue;
            };
            let pulls: Vec<_> = served.pulls.iter().take(tracked.mine).cloned().collect();
            if pulls.is_empty() {
                continue;
            }
            let states = pulls.iter().filter_map(|pull| pull.checks.state);
            let checks = states.max_by_key(|state| match state {
                CheckState::Passing => 0,
                CheckState::Pending => 1,
                CheckState::Failing => 2,
            });
            summaries.push(PullSummary {
                project_id: id.clone(),
                pulls,
                checks,
            });
        }
        summaries
    }
}

/// An unwatched project is read for the summaries when it was never read or its status
/// is older than a settled one would be.
fn summary_wants(tracked: &Tracked, now: Instant) -> bool {
    tracked.read_at.is_none_or(|at| now - at > SETTLED)
}

/// Whether an open pull request has a check still running.
fn running(status: &RepositoryStatus) -> bool {
    status.forge.served().is_some_and(|served| {
        served.pulls.iter().any(|pull| {
            matches!(pull.state, PullState::Open | PullState::Draft) && pull.checks.running > 0
        })
    })
}

fn backoff(failures: u32) -> Duration {
    BACKOFF
        .saturating_mul(1 << failures.saturating_sub(1).min(10))
        .min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::project::{Checks, ForgeStatus, PullStatus};

    fn pull(number: u64, running: u32, state: Option<CheckState>) -> PullStatus {
        PullStatus {
            number,
            title: "t".to_owned(),
            state: PullState::Open,
            branch: "b".to_owned(),
            author: String::new(),
            created: None,
            updated: None,
            added: 0,
            removed: 0,
            files: None,
            url: String::new(),
            checks_url: String::new(),
            checks: Checks {
                total: running,
                running,
                state,
                ..Checks::default()
            },
        }
    }

    fn reading(pulls: Vec<PullStatus>, mine: usize, trouble: bool) -> Reading {
        Reading {
            status: RepositoryStatus {
                forge: Forge::Served(Box::new(ForgeStatus {
                    plugin: "github".to_owned(),
                    name: "GitHub".to_owned(),
                    repository: "o/r".to_owned(),
                    pull_prefix: "https://github.com/o/r/pull/".to_owned(),
                    pulls,
                    unavailable: None,
                })),
                ..RepositoryStatus::default()
            },
            mine,
            trouble,
        }
    }

    #[test]
    fn a_watched_project_is_read_once_at_a_time_and_waits_by_its_checks() {
        let tracker = Tracker::default();
        assert_eq!(tracker.watch(1, "p"), None);
        assert_eq!(tracker.due(), ["p"]);
        assert!(tracker.due().is_empty(), "one read in flight at a time");

        let running = vec![pull(1, 2, Some(CheckState::Pending))];
        let (_, changed) = tracker.finish("p", reading(running.clone(), 1, false));
        assert!(changed);
        let (_, changed) = tracker.finish("p", reading(running, 1, false));
        assert!(!changed, "the same status is no news");
        let due = tracker.0.lock().projects["p"].due;
        assert!(due > Instant::now() + RUNNING - Duration::from_secs(1));
        assert!(due < Instant::now() + SETTLED - Duration::from_secs(60));

        tracker.finish("p", reading(vec![pull(1, 0, None)], 1, false));
        let due = tracker.0.lock().projects["p"].due;
        assert!(due > Instant::now() + SETTLED - Duration::from_secs(1));

        tracker.changed("p");
        assert_eq!(tracker.due(), ["p"]);
    }

    #[test]
    fn trouble_keeps_the_known_pulls_and_backs_off() {
        let tracker = Tracker::default();
        tracker.watch(1, "p");
        tracker.finish("p", reading(vec![pull(7, 0, None)], 1, false));
        let (status, changed) = tracker.finish("p", reading(Vec::new(), 0, true));
        assert_eq!(status.forge.served().unwrap().pulls[0].number, 7);
        assert!(!changed);
        tracker.finish("p", reading(Vec::new(), 0, true));
        let due = tracker.0.lock().projects["p"].due;
        assert!(due > Instant::now() + BACKOFF * 2 - Duration::from_secs(1));
        assert_eq!(backoff(40), MAX_BACKOFF);
    }

    #[test]
    fn nobody_watching_means_no_reads_beyond_the_summaries() {
        let tracker = Tracker::default();
        tracker.watch(1, "p");
        tracker.forget(1);
        // Never read: the summaries want it once.
        assert_eq!(tracker.due(), ["p"]);
        tracker.finish("p", reading(vec![pull(1, 3, None)], 1, false));
        tracker.changed("p");
        assert!(tracker.due().is_empty(), "read lately, and nobody watches");
    }

    #[test]
    fn summaries_count_the_branch_pulls_and_take_the_worst_checks() {
        let tracker = Tracker::default();
        let ids = ["p".to_owned(), "q".to_owned()];
        assert!(tracker.summaries(&ids).is_empty());
        let pulls = vec![
            pull(1, 0, Some(CheckState::Passing)),
            pull(2, 1, Some(CheckState::Pending)),
            pull(3, 0, Some(CheckState::Failing)),
        ];
        tracker.finish("p", reading(pulls, 2, false));
        let summaries = tracker.summaries(&ids);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pulls.len(), 2);
        assert_eq!(summaries[0].checks, Some(CheckState::Pending));
        // Only one unwatched project is read per tick.
        assert_eq!(tracker.due(), ["q"]);
    }
}
