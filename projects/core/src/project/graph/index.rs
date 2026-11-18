//! The indexer: keeps the graph of saved projects current, in the background. One worker
//! for the whole core takes one project at a time from a queue; what it does with a
//! project is in `refresh`. Nothing here is on the model's path: a request is put in the
//! queue and returns, and with `limits.project_graph` off nothing is queued at all.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use tokio::{sync::Notify, task::JoinHandle};
use tracing::warn;

use super::refresh::{Limits, Refreshed, Seams, Work};
use crate::state::{State, graph::GraphCounts};

/// How long `project_graph` waits for the changed paths of its project to be indexed.
// The tool is task G4 of the design.
#[allow(dead_code)]
pub const PATHS_WAIT: Duration = Duration::from_millis(300);
/// More changed paths than this waiting for one project become a refresh.
const MAX_QUEUED_PATHS: usize = 1_000;

/// A thread's folder as the indexer walks it. Only a folder that is exactly a saved
/// project's `dir` is indexed; that is checked by the worker, against the database.
#[derive(Debug, Clone)]
pub struct Folder {
    /// Canonical, as `ToolScope::cwd` gives it.
    pub root: PathBuf,
    /// Folders inside it that are not the project's: the config folder, a thread workspace.
    pub skip: Vec<PathBuf>,
}

/// A file that changed: its path relative to the project, `/` between parts.
#[derive(Debug, Clone)]
pub struct Changed {
    pub path: String,
    /// SHA-256 of what it holds now, when the caller hashed it: a file whose indexed
    /// hash is this one is not opened.
    pub hash: Option<String>,
}

/// Where a project's index stands, in memory: nothing of it outlives the core.
// Read through `Indexer::status`, which the tool will call (task G4 of the design).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not indexed: the switch is off, the folder is not a saved project, or no refresh
    /// was asked for in this run.
    Off,
    /// The first refresh of this run has not finished; what is stored may be stale.
    Building,
    /// A refresh finished in this run. The counts are those of that refresh.
    Ready(GraphCounts),
    /// Past a cap; its rows were deleted.
    TooLarge,
}

pub struct Indexer {
    state: Arc<State>,
    /// `limits.project_graph`, read on every request so a change of the config holds.
    on: Arc<dyn Fn() -> bool + Send + Sync>,
    seams: Seams,
    limits: Limits,
    queue: Mutex<Queue>,
    /// Wakes the worker.
    queued: Notify,
    /// Told when a job's paths are done and when a job ends.
    settled: Notify,
}

#[derive(Default)]
struct Queue {
    /// Projects with a job, first in first out.
    waiting: VecDeque<String>,
    jobs: HashMap<String, Job>,
    /// The project the worker has, and whether its changed paths are still to be done.
    running: Option<(String, bool)>,
    status: HashMap<String, Status>,
    /// Projects where the ledger could not say what a command changed.
    unrecorded: HashSet<String>,
}

/// What waits for one project: requests that come while it waits are merged into it.
struct Job {
    folder: Folder,
    refresh: bool,
    /// Changed paths with their hash, when known.
    paths: BTreeMap<String, Option<String>>,
}

impl Indexer {
    /// Starts the worker. `on` says whether `limits.project_graph` is on. The worker
    /// runs until its task is aborted; a batch is one transaction, so stopping it
    /// anywhere leaves whole files.
    pub fn start(
        state: Arc<State>,
        on: impl Fn() -> bool + Send + Sync + 'static,
    ) -> (Arc<Self>, JoinHandle<()>) {
        let indexer = Arc::new(Self::new(
            state,
            Arc::new(on),
            Seams::real(),
            Limits::default(),
        ));
        let worker = tokio::spawn(Arc::clone(&indexer).run());
        (indexer, worker)
    }

    pub(super) fn new(
        state: Arc<State>,
        on: Arc<dyn Fn() -> bool + Send + Sync>,
        seams: Seams,
        limits: Limits,
    ) -> Self {
        Self {
            state,
            on,
            seams,
            limits,
            queue: Mutex::default(),
            queued: Notify::new(),
            settled: Notify::new(),
        }
    }

    /// Asks for the project to be walked and what changed to be indexed. Dropped when a
    /// refresh of the same project already waits.
    pub fn refresh(&self, folder: Folder) {
        self.enqueue(folder, true, Vec::new());
    }

    /// Asks for these files to be indexed again, without a walk.
    pub fn changed(&self, folder: Folder, paths: Vec<Changed>) {
        if !paths.is_empty() {
            self.enqueue(folder, false, paths);
        }
    }

    /// The ledger could not say what a command changed in the project (it is past the
    /// ledger's caps), so the end of the turn refreshes it.
    pub fn unrecorded(&self, root: &Path) {
        if (self.on)()
            && let Some(project) = root.to_str()
        {
            self.queue.lock().unrecorded.insert(project.to_owned());
        }
    }

    /// A turn ended. With the ledger recording (`recorded`) the edits came as they were
    /// made; without it, or where it gave up, the project is refreshed.
    pub fn turn_ended(&self, folder: Folder, recorded: bool) {
        let gave_up = folder
            .root
            .to_str()
            .is_some_and(|project| self.queue.lock().unrecorded.remove(project));
        if !recorded || gave_up {
            self.refresh(folder);
        }
    }

    /// Waits, `limit` at most, until no changed path of the project is waiting or being
    /// indexed. A refresh is not waited for.
    // Called by the `project_graph` tool before it answers (task G4 of the design).
    #[allow(dead_code)]
    pub async fn settle(&self, root: &Path, limit: Duration) {
        let Some(project) = root.to_str() else {
            return;
        };
        let settled = async {
            loop {
                // Listening before the look, so an end between the two is not missed.
                let told = self.settled.notified();
                tokio::pin!(told);
                told.as_mut().enable();
                if !self.has_paths(project) {
                    return;
                }
                told.await;
            }
        };
        let _ = tokio::time::timeout(limit, settled).await;
    }

    /// Where the project's index stands: a read of memory, no query.
    // The `available` check of the `project_graph` tool (task G4 of the design).
    #[allow(dead_code)]
    pub fn status(&self, root: &Path) -> Status {
        if !(self.on)() {
            return Status::Off;
        }
        let queue = self.queue.lock();
        root.to_str()
            .and_then(|project| queue.status.get(project).copied())
            .unwrap_or(Status::Off)
    }

    fn enqueue(&self, folder: Folder, refresh: bool, paths: Vec<Changed>) {
        if !(self.on)() {
            return;
        }
        // A saved project's folder is stored as text, so another spelling is not one.
        let Some(project) = folder.root.to_str().map(str::to_owned) else {
            return;
        };
        {
            let mut queue = self.queue.lock();
            let queue = &mut *queue;
            let job = queue.jobs.entry(project.clone()).or_insert_with(|| {
                queue.waiting.push_back(project);
                Job {
                    folder: Folder {
                        root: folder.root,
                        skip: Vec::new(),
                    },
                    refresh: false,
                    paths: BTreeMap::new(),
                }
            });
            // Each thread has its own workspace to leave out.
            for skip in folder.skip {
                if !job.folder.skip.contains(&skip) {
                    job.folder.skip.push(skip);
                }
            }
            job.refresh |= refresh;
            job.paths
                .extend(paths.into_iter().map(|file| (file.path, file.hash)));
            if job.paths.len() > MAX_QUEUED_PATHS {
                job.paths.clear();
                job.refresh = true;
            }
        }
        self.queued.notify_one();
    }

    fn has_paths(&self, project: &str) -> bool {
        let queue = self.queue.lock();
        let waiting = queue
            .jobs
            .get(project)
            .is_some_and(|job| !job.paths.is_empty());
        let running = queue
            .running
            .as_ref()
            .is_some_and(|(running, paths)| *paths && running == project);
        waiting || running
    }

    async fn run(self: Arc<Self>) {
        loop {
            if !self.step().await {
                self.queued.notified().await;
            }
        }
    }

    /// Does the job that waited longest. False when none waits.
    pub(super) async fn step(&self) -> bool {
        let Some((project, job)) = self.take() else {
            return false;
        };
        self.work(&project, job).await;
        self.queue.lock().running = None;
        self.settled.notify_waiters();
        true
    }

    fn take(&self) -> Option<(String, Job)> {
        let mut queue = self.queue.lock();
        let project = queue.waiting.pop_front()?;
        let job = queue.jobs.remove(&project)?;
        queue.running = Some((project.clone(), !job.paths.is_empty()));
        Some((project, job))
    }

    async fn work(&self, project: &str, job: Job) {
        if !(self.on)() {
            return;
        }
        match self.state.graph_project(project).await {
            Ok(true) => {}
            // Deny by default: an unsaved folder, or a folder inside a project.
            Ok(false) => {
                self.queue.lock().status.remove(project);
                return;
            }
            Err(error) => {
                warn!(project, "cannot look for the project of a graph: {error:#}");
                return;
            }
        }
        let work = Work {
            state: &self.state,
            project,
            folder: &job.folder,
            seams: &self.seams,
            limits: self.limits,
        };
        let too_large = self.queue.lock().status.get(project) == Some(&Status::TooLarge);
        if !job.paths.is_empty() && !too_large {
            work.update(job.paths).await;
        }
        if let Some((_, paths)) = &mut self.queue.lock().running {
            *paths = false;
        }
        self.settled.notify_waiters();
        if !job.refresh {
            return;
        }
        {
            let mut queue = self.queue.lock();
            // What a refresh of this run stored stays offered while the next one runs.
            if !matches!(queue.status.get(project), Some(Status::Ready(_))) {
                queue.status.insert(project.to_owned(), Status::Building);
            }
        }
        let status = match work.refresh().await {
            Ok(Refreshed::Done(counts)) => Some(Status::Ready(counts)),
            Ok(Refreshed::TooLarge) => Some(Status::TooLarge),
            Err(error) => {
                warn!(project, "cannot refresh the project graph: {error:#}");
                None
            }
        };
        let mut queue = self.queue.lock();
        match status {
            Some(status) => queue.status.insert(project.to_owned(), status),
            None => queue.status.remove(project),
        };
    }
}
