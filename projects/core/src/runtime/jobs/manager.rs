//! Runtime-owned processes. A tool call launches them; only the runtime owns their
//! cancellation and completion, so ending that call or turn does not drop the child.

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use parking_lot::Mutex;

use anyhow::{Result, ensure};
use tokio::sync::{Mutex as AsyncMutex, watch};
use tracing::warn;
use uuid::Uuid;

use super::lease::{JobLease, Owner};
use crate::{
    runtime::agent::{Input, Mailboxes},
    state::{BackgroundJob, JobSummary, State},
    tools::console::process::{CommandEnv, HeadTail, Running},
};

pub use dusk_protocol::thread::JobInfo;

const MAX_RUNNING: usize = 64;
const MAX_RUNNING_PER_THREAD: usize = 8;
const MAX_COMMAND_BYTES: usize = 16 * 1024;

/// Where and how a background command starts.
pub struct Launch<'a> {
    pub cwd: &'a std::path::Path,
    pub confinement: Option<&'a crate::platform::Confinement>,
    pub env: &'a CommandEnv,
}

pub struct JobManager {
    pub state: Arc<State>,
    mailboxes: Arc<Mailboxes>,
    live: AsyncMutex<HashMap<String, Arc<RunningJob>>>,
    lease: Arc<JobLease>,
    /// Foreground commands the user may send to the background, by tool call ID: the
    /// thread they run in and the switch their tool watches.
    foreground: Mutex<HashMap<String, (String, watch::Sender<bool>)>>,
}

/// A foreground command's place in the list, for as long as it runs.
pub struct Foreground {
    manager: Weak<JobManager>,
    call_id: String,
    request: watch::Receiver<bool>,
}

impl Foreground {
    /// Turns true when the user sends the command to the background.
    pub fn request(&self) -> watch::Receiver<bool> {
        self.request.clone()
    }
}

impl Drop for Foreground {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.upgrade() {
            manager.foreground.lock().remove(&self.call_id);
        }
    }
}

struct RunningJob {
    owner: String,
    cancel: watch::Sender<bool>,
    done: watch::Sender<bool>,
    stdout: Arc<Mutex<HeadTail>>,
    stderr: Arc<Mutex<HeadTail>>,
}

#[derive(Clone)]
pub struct JobOwner {
    pub manager: Arc<JobManager>,
    pub thread_id: String,
}

impl JobManager {
    pub async fn new(
        state: Arc<State>,
        mailboxes: Arc<Mailboxes>,
        database: &std::path::Path,
    ) -> Result<Arc<Self>> {
        let lease = Arc::new(JobLease::take(database)?);
        for (id, path) in state.unfinished_job_owners().await? {
            if let Owner::Abandoned(abandoned) = JobLease::inspect(&id, &path) {
                // Keep the OS lock while fencing recovery by the immutable owner ID.
                state.interrupt_jobs(&abandoned.id).await?;
                drop(abandoned);
            }
        }
        let needed = state
            .unfinished_job_owners()
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        lease.sweep(&needed);
        Ok(Arc::new(Self {
            state,
            mailboxes,
            lease,
            live: AsyncMutex::new(HashMap::new()),
            foreground: Mutex::default(),
        }))
    }

    pub fn status(&self, job: &BackgroundJob) -> String {
        self.status_of(
            &job.status,
            job.owner_runtime_id.as_deref(),
            job.owner_lease_path.as_deref(),
        )
    }

    fn status_of(&self, status: &str, owner: Option<&str>, lease_path: Option<&str>) -> String {
        if status != "running" {
            return status.to_owned();
        }
        let (Some(id), Some(path)) = (owner, lease_path) else {
            return "unknown (owner unavailable)".into();
        };
        if id == self.lease.id {
            return "running".into();
        }
        match JobLease::inspect(id, path) {
            Owner::Live => "running (another runtime)".into(),
            Owner::Abandoned(_) | Owner::Unknown => "unknown (owner unavailable)".into(),
        }
    }

    /// The thread's jobs, oldest first, as a client shows them.
    pub async fn list(&self, thread_id: &str) -> Result<Vec<JobInfo>> {
        let summaries = self.state.job_summaries(thread_id).await?;
        let live = self.live.lock().await;
        Ok(summaries
            .into_iter()
            .map(|job: JobSummary| JobInfo {
                status: self.status_of(
                    &job.status,
                    job.owner_runtime_id.as_deref(),
                    job.owner_lease_path.as_deref(),
                ),
                running: job.status == "running",
                stoppable: live.contains_key(&job.job_id),
                job_id: job.job_id,
                command: job.command,
                started_at: job.created_at,
                finished_at: job.finished_at,
            })
            .collect())
    }

    /// What a job printed: live while it runs, the stored result after.
    pub async fn output(&self, thread_id: &str, id: &str) -> Result<String> {
        let job = self.state.job(thread_id, id).await?;
        Ok(self.printed(thread_id, id, job).await)
    }

    pub async fn printed(&self, thread_id: &str, id: &str, job: BackgroundJob) -> String {
        let live = self.live.lock().await;
        match live.get(id).filter(|job| job.owner == thread_id) {
            Some(job) => format!(
                "stdout:\n{}\nstderr:\n{}",
                job.stdout.lock().text(),
                job.stderr.lock().text()
            ),
            None => job.output,
        }
    }

    pub async fn wake_state(&self, thread_id: &str) -> Result<(bool, bool)> {
        self.state.job_wake_state(thread_id, &self.lease.id).await
    }

    pub async fn has_running(&self, thread_id: &str) -> bool {
        self.live
            .lock()
            .await
            .values()
            .any(|job| job.owner == thread_id)
    }

    pub async fn start(
        self: &Arc<Self>,
        owner: &str,
        command: &str,
        launch: Launch<'_>,
        timeout_seconds: u64,
        call_id: Option<&str>,
    ) -> Result<String> {
        let running = || Running::start(command, launch.cwd, launch.confinement, launch.env);
        self.launch(owner, command, call_id, running, timeout_seconds)
            .await
    }

    /// Takes over a foreground command the user sent to the background, as it runs.
    pub async fn adopt(
        self: &Arc<Self>,
        owner: &str,
        command: &str,
        call_id: Option<&str>,
        running: Running,
        timeout_seconds: u64,
    ) -> Result<String> {
        let mut running = Some(running);
        let take = || {
            running
                .take()
                .ok_or_else(|| anyhow::anyhow!("command taken twice"))
        };
        self.launch(owner, command, call_id, take, timeout_seconds)
            .await
    }

    /// Lists a foreground command, so the user can send it to the background.
    pub fn foreground(self: &Arc<Self>, owner: &str, call_id: &str) -> Foreground {
        let (switch, request) = watch::channel(false);
        self.foreground
            .lock()
            .insert(call_id.to_owned(), (owner.to_owned(), switch));
        Foreground {
            manager: Arc::downgrade(self),
            call_id: call_id.to_owned(),
            request,
        }
    }

    /// Asks the foreground command of `call_id` to go to the background, if it may.
    pub async fn send_to_background(&self, owner: &str, call_id: &str) -> Result<()> {
        {
            let live = self.live.lock().await;
            ensure!(
                live.len() < MAX_RUNNING,
                "at most {MAX_RUNNING} background jobs may run"
            );
            ensure!(
                live.values().filter(|job| job.owner == owner).count() < MAX_RUNNING_PER_THREAD,
                "at most {MAX_RUNNING_PER_THREAD} background jobs may run in this thread"
            );
        }
        let foreground = self.foreground.lock();
        let (thread, switch) = foreground.get(call_id).ok_or_else(|| {
            anyhow::anyhow!("the command has ended or cannot go to the background")
        })?;
        ensure!(thread == owner, "the command belongs to another thread");
        switch.send_replace(true);
        Ok(())
    }

    /// Records the job, starts or takes its process, and leaves it to a runtime task.
    async fn launch(
        self: &Arc<Self>,
        owner: &str,
        command: &str,
        call_id: Option<&str>,
        process: impl FnOnce() -> Result<Running>,
        timeout_seconds: u64,
    ) -> Result<String> {
        ensure!(
            command.len() <= MAX_COMMAND_BYTES,
            "background command exceeds 16 KiB"
        );
        ensure!(!self.mailboxes.is_closed(), "the runtime is shutting down");
        let mut live = self.live.lock().await;
        ensure!(
            live.len() < MAX_RUNNING,
            "at most {MAX_RUNNING} background jobs may run"
        );
        ensure!(
            live.values().filter(|job| job.owner == owner).count() < MAX_RUNNING_PER_THREAD,
            "at most {MAX_RUNNING_PER_THREAD} background jobs may run in this thread"
        );
        let job_id = Uuid::now_v7().to_string();
        let mut unstarted = UnstartedJob {
            state: Arc::clone(&self.state),
            job_id: job_id.clone(),
            armed: true,
            lease: Arc::clone(&self.lease),
        };
        self.state
            .create_job_for_call(
                owner,
                &job_id,
                command,
                &self.lease.id,
                self.lease.stored_path()?,
                call_id,
            )
            .await?;
        ensure!(!self.mailboxes.is_closed(), "the runtime is shutting down");
        let running = match process() {
            Ok(running) => running,
            Err(error) => {
                self.state
                    .remove_unstarted_job(&job_id, &self.lease.id)
                    .await?;
                return Err(error);
            }
        };
        let (cancel, cancelled) = watch::channel(false);
        let (done, _) = watch::channel(false);
        let job = Arc::new(RunningJob {
            owner: owner.to_owned(),
            cancel,
            done,
            stdout: Arc::clone(&running.stdout),
            stderr: Arc::clone(&running.stderr),
        });
        live.insert(job_id.clone(), Arc::clone(&job));
        // No await between taking the process, handing it to the runtime task and
        // returning its ID. Cancellation of the launching tool cannot strand it.
        let manager = Arc::downgrade(self);
        let state = Arc::clone(&self.state);
        let mailboxes = Arc::clone(&self.mailboxes);
        let id = job_id.clone();
        let lease = Arc::clone(&self.lease);
        tokio::spawn(async move {
            let (status, output) = execute(running, cancelled, timeout_seconds).await;
            // A transient state failure must not lose the result. Keep ownership and
            // bounded output until storage succeeds, or shutdown closes the database.
            loop {
                match state.finish_job(&id, &lease.id, status, &output).await {
                    Ok(()) => break,
                    Err(_) if state.pool.is_closed() => {
                        warn!(job_id = %id, "database closed before job completion could be saved");
                        job.done.send_replace(true);
                        return;
                    }
                    Err(error) => {
                        warn!(job_id = %id, "cannot save background job completion: {error:#}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            if let Some(manager) = Weak::upgrade(&manager) {
                manager.live.lock().await.remove(&id);
            }
            // The durable event comes before the wake, and Sent never interrupts an
            // in-flight model/tool call. A closed thread reads it when next resumed.
            if let Some(mailbox) = mailboxes.get(&job.owner).await {
                mailbox.send(Input::Sent);
            }
            job.done.send_replace(true);
        });
        unstarted.armed = false;
        Ok(job_id)
    }

    pub async fn cancel(&self, owner: &str, id: &str) -> Result<String> {
        let stored = self.state.job(owner, id).await?;
        if let Some(job) = self.live.lock().await.get(id) {
            ensure!(
                job.owner == owner,
                "background job belongs to another thread"
            );
            job.cancel.send_replace(true);
            return Ok(format!("cancellation requested for job {id}"));
        }
        ensure!(
            stored.status != "running",
            "job {id}: {}; this runtime cannot cancel its process",
            self.status(&stored)
        );
        Ok(format!("job {id} is already finished"))
    }

    pub async fn shutdown(&self) {
        let jobs: Vec<_> = self.live.lock().await.values().cloned().collect();
        for job in &jobs {
            job.cancel.send_replace(true);
        }
        let wait = async {
            for job in jobs {
                let mut done = job.done.subscribe();
                while !*done.borrow_and_update() {
                    if done.changed().await.is_err() {
                        break;
                    }
                }
            }
        };
        // Processes are killed/reaped before completion persistence, even when the
        // database is unavailable. The pool is closed by Runtime after this grace.
        if tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .is_err()
        {
            warn!("background jobs did not finish saving during shutdown");
        }
    }
}

// Dropping the launching tool during its database await cannot leave a phantom
// running record. Cleanup is serialized after any rolled-back/committed transaction.
struct UnstartedJob {
    state: Arc<State>,
    job_id: String,
    armed: bool,
    lease: Arc<JobLease>,
}
impl Drop for UnstartedJob {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let state = Arc::clone(&self.state);
        let id = self.job_id.clone();
        let lease = Arc::clone(&self.lease);
        tokio::spawn(async move {
            if let Err(error) = state.remove_unstarted_job(&id, &lease.id).await {
                warn!(job_id = %id, "cannot remove unstarted job: {error:#}");
            }
        });
    }
}

impl Drop for JobManager {
    fn drop(&mut self) {
        for job in self.live.get_mut().values() {
            job.cancel.send_replace(true);
        }
    }
}

async fn execute(
    mut running: Running,
    mut cancelled: watch::Receiver<bool>,
    timeout_seconds: u64,
) -> (&'static str, String) {
    let deadline = tokio::time::sleep(Duration::from_secs(timeout_seconds));
    tokio::pin!(deadline);
    let (status, exit) = tokio::select! {
        // Once an outcome wins, a later cancellation cannot overwrite it.
        exit = running.child.wait() => ("completed", Some(exit)),
        _ = cancelled.wait_for(|value| *value) => ("cancelled", None),
        _ = &mut deadline => ("timed_out", None),
    };
    let (stdout, stderr, failure) = running.collect().await;
    let exited_cleanly = matches!(&exit, Some(Ok(exit)) if exit.success());
    let mut output = match exit {
        Some(Ok(exit)) => format!(
            "exit code: {}",
            exit.code().map_or_else(
                || "none (terminated by a signal)".into(),
                |code| code.to_string()
            )
        ),
        Some(Err(error)) => format!("cannot wait for command: {error}"),
        None => "exit code: none (stopped)".into(),
    };
    let failed = failure.is_some() || output.starts_with("cannot wait");
    for (name, text) in [("stdout", stdout), ("stderr", stderr)] {
        if !text.is_empty() {
            output.push_str(&format!("\n{name}:\n{text}"));
        }
    }
    if let Some(failure) = failure {
        output.push_str(&format!("\n{failure}"));
    }
    // Cancelled and timed-out jobs keep their own status; a finished one is judged by
    // its exit code, which the model sees in the output as well.
    let status = if status == "completed" && (failed || !exited_cleanly) {
        "failed"
    } else {
        status
    };
    (status, output)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::testing::temp_dir;

    /// A plain command in the current folder.
    fn launch() -> Launch<'static> {
        static ENV: std::sync::LazyLock<CommandEnv> = std::sync::LazyLock::new(CommandEnv::default);
        Launch {
            cwd: Path::new("."),
            confinement: None,
            env: &ENV,
        }
    }

    async fn open(name: &str) -> (Arc<State>, PathBuf) {
        let database = temp_dir(name).join("state.db");
        let state = Arc::new(State::open(&database).await.unwrap());
        state.create_thread("thread", "test").await.unwrap();
        (state, database)
    }

    async fn manager(state: &Arc<State>, database: &Path) -> Arc<JobManager> {
        let mailboxes = Arc::new(Mailboxes::default());
        JobManager::new(Arc::clone(state), mailboxes, database)
            .await
            .unwrap()
    }

    async fn finished(state: &State, id: &str) -> BackgroundJob {
        for _ in 0..100 {
            let job = state.job("thread", id).await.unwrap();
            if job.status != "running" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the job did not finish");
    }

    async fn announced(state: &State, id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM pending_messages WHERE from_job_id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .unwrap()
    }

    /// The user's switch reaches only the thread's own command, and the command taken
    /// over goes on running and ends as a job with all its output.
    #[tokio::test]
    async fn a_command_sent_to_the_background_finishes_as_a_job() {
        let (state, database) = open("job-adopt").await;
        let manager = manager(&state, &database).await;
        let handed = manager.foreground("thread", "call_1");
        let mut request = handed.request();
        let command = "echo early; sleep 0.3; echo late";
        let running =
            Running::start(command, Path::new("."), None, &CommandEnv::default()).unwrap();
        assert!(manager.send_to_background("other", "call_1").await.is_err());
        manager
            .send_to_background("thread", "call_1")
            .await
            .unwrap();
        request.wait_for(|asked| *asked).await.unwrap();
        let id = manager
            .adopt("thread", command, None, running, 10)
            .await
            .unwrap();
        drop(handed);
        let gone = manager.send_to_background("thread", "call_1").await;
        assert!(gone.is_err(), "the switch goes with its command");
        let job = finished(&state, &id).await;
        assert_eq!(job.status, "completed");
        assert!(job.output.contains("early") && job.output.contains("late"));
    }

    #[tokio::test]
    async fn a_finished_job_is_announced_once() {
        let (state, database) = open("job-once").await;
        let manager = manager(&state, &database).await;
        let id = manager
            .start("thread", "echo hi", launch(), 10, None)
            .await
            .unwrap();
        let job = finished(&state, &id).await;
        assert_eq!(job.status, "completed");
        assert!(job.output.contains("hi"));
        // A replayed outcome changes nothing and queues nothing more.
        state
            .finish_job(&id, &manager.lease.id, "failed", "again")
            .await
            .unwrap();
        assert_eq!(state.job("thread", &id).await.unwrap().status, "completed");
        assert_eq!(announced(&state, &id).await, 1);
    }

    #[tokio::test]
    async fn a_nonzero_exit_fails_the_job() {
        let (state, database) = open("job-exit").await;
        let manager = manager(&state, &database).await;
        let id = manager
            .start("thread", "echo bad >&2; exit 3", launch(), 10, None)
            .await
            .unwrap();
        let job = finished(&state, &id).await;
        assert_eq!(job.status, "failed");
        assert!(job.output.contains("exit code: 3") && job.output.contains("bad"));
    }

    #[tokio::test]
    async fn cancelling_stops_the_process_and_says_so() {
        let (state, database) = open("job-cancel").await;
        let manager = manager(&state, &database).await;
        let id = manager
            .start("thread", "sleep 30", launch(), 60, None)
            .await
            .unwrap();
        manager.cancel("thread", &id).await.unwrap();
        assert_eq!(finished(&state, &id).await.status, "cancelled");
        assert!(manager.cancel("other", &id).await.is_err());
    }

    #[tokio::test]
    async fn an_abandoned_owner_is_interrupted_and_its_lease_swept() {
        let (state, database) = open("job-abandoned").await;
        // A runtime that is gone: its lease was taken and let go, the file stays.
        let gone = JobLease::take(&database).unwrap();
        let path = gone.stored_path().unwrap().to_owned();
        state
            .create_job("thread", "job", "sleep 99", &gone.id, &path)
            .await
            .unwrap();
        drop(gone);
        // A test beside this one that starts a command forks, and until the child execs
        // it holds every descriptor of this process, the dropped lease's among them. A
        // start in that moment sees a live owner and leaves the job for the next start,
        // which is what a real runtime does too: so both the interruption and the sweep
        // are given a few starts to happen.
        for _ in 0..20 {
            manager(&state, &database).await;
            let interrupted = state.job("thread", "job").await.unwrap().status == "interrupted";
            if interrupted && !Path::new(&path).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(
            state.job("thread", "job").await.unwrap().status,
            "interrupted"
        );
        // However many starts it took, the end is told once.
        assert_eq!(announced(&state, "job").await, 1);
        assert!(!Path::new(&path).exists(), "the lease was not swept");
    }

    #[tokio::test]
    async fn a_live_owner_is_left_alone() {
        let (state, database) = open("job-live").await;
        let live = JobLease::take(&database).unwrap();
        let path = live.stored_path().unwrap().to_owned();
        state
            .create_job("thread", "job", "sleep 99", &live.id, &path)
            .await
            .unwrap();
        let manager = manager(&state, &database).await;
        let job = state.job("thread", "job").await.unwrap();
        assert_eq!(job.status, "running");
        assert_eq!(manager.status(&job), "running (another runtime)");
        assert_eq!(announced(&state, "job").await, 0);
        assert!(Path::new(&path).exists());
    }

    #[tokio::test]
    async fn a_client_lists_a_running_job_and_reads_its_output() {
        let (state, database) = open("job-list").await;
        let manager = manager(&state, &database).await;
        let id = manager
            .start("thread", "echo first; sleep 30", launch(), 60, None)
            .await
            .unwrap();
        let mut output = String::new();
        for _ in 0..100 {
            output = manager.output("thread", &id).await.unwrap();
            if output.contains("first") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(output.contains("first"), "live output: {output}");
        let jobs = manager.list("thread").await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].running && jobs[0].stoppable && jobs[0].status == "running");
        assert!(manager.list("other").await.unwrap().is_empty());
        manager.cancel("thread", &id).await.unwrap();
        finished(&state, &id).await;
        let jobs = manager.list("thread").await.unwrap();
        assert!(!jobs[0].running && !jobs[0].stoppable);
        assert_eq!(jobs[0].status, "cancelled");
        assert!(jobs[0].finished_at.is_some());
    }
}
