//! Runtime-owned processes. A tool call launches them; only the runtime owns their
//! cancellation and completion, so ending that call or turn does not drop the child.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
    sync::{Mutex as AsyncMutex, watch},
};
use tracing::warn;
use uuid::Uuid;

use super::{
    job_lease::{JobLease, Owner},
    run_command::{ProcessGroup, spawn},
};
use crate::{
    BoxFuture,
    runtime::agent::{Input, Mailboxes},
    state::{BackgroundJob, State},
    tools::{Tool, ToolContext, ToolKind, builtin::parse_arguments},
    util::sync::lock,
};

const MAX_RUNNING: usize = 64;
const MAX_RUNNING_PER_THREAD: usize = 8;
const MAX_COMMAND_BYTES: usize = 16 * 1024;
const MAX_STREAM_BYTES: usize = 32 * 1024;

pub struct JobManager {
    state: Arc<State>,
    mailboxes: Arc<Mailboxes>,
    live: AsyncMutex<HashMap<String, Arc<RunningJob>>>,
    lease: Arc<JobLease>,
}

struct RunningJob {
    owner: String,
    cancel: watch::Sender<bool>,
    done: watch::Sender<bool>,
    stdout: Arc<Mutex<Capture>>,
    stderr: Arc<Mutex<Capture>>,
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
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
        }))
    }

    fn status(&self, job: &BackgroundJob) -> String {
        if job.status != "running" {
            return job.status.clone();
        }
        let (Some(id), Some(path)) = (&job.owner_runtime_id, &job.owner_lease_path) else {
            return "unknown (owner unavailable)".into();
        };
        if id == &self.lease.id {
            return "running".into();
        }
        match JobLease::inspect(id, path) {
            Owner::Live => "running (another runtime)".into(),
            Owner::Abandoned(_) | Owner::Unknown => "unknown (owner unavailable)".into(),
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
        cwd: &std::path::Path,
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
            .create_job(
                owner,
                &job_id,
                command,
                &self.lease.id,
                self.lease.stored_path()?,
            )
            .await?;
        ensure!(!self.mailboxes.is_closed(), "the runtime is shutting down");
        let child = match spawn(command, cwd) {
            Ok(child) => child,
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
            stdout: Arc::new(Mutex::new(Capture::default())),
            stderr: Arc::new(Mutex::new(Capture::default())),
        });
        live.insert(job_id.clone(), Arc::clone(&job));
        // No await between spawning the child, handing it to the runtime task and
        // returning its ID. Cancellation of the launching tool cannot strand it.
        let manager = Arc::downgrade(self);
        let state = Arc::clone(&self.state);
        let mailboxes = Arc::clone(&self.mailboxes);
        let id = job_id.clone();
        let lease = Arc::clone(&self.lease);
        tokio::spawn(async move {
            let (status, output) = execute(child, &job, cancelled, timeout_seconds).await;
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

    async fn cancel(&self, owner: &str, id: &str) -> Result<String> {
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
    mut child: Child,
    job: &RunningJob,
    mut cancelled: watch::Receiver<bool>,
    timeout_seconds: u64,
) -> (&'static str, String) {
    let mut group = ProcessGroup(child.id());
    let stdout = child.stdout.take().expect("spawn captures stdout");
    let stderr = child.stderr.take().expect("spawn captures stderr");
    let (reaped, drain) = watch::channel(false);
    let wait = async {
        let deadline = tokio::time::sleep(Duration::from_secs(timeout_seconds));
        tokio::pin!(deadline);
        let (status, exit) = tokio::select! {
            // Once an outcome wins, a later cancellation cannot overwrite it.
            exit = child.wait() => ("completed", Some(exit)),
            _ = cancelled.wait_for(|value| *value) => ("cancelled", None),
            _ = &mut deadline => ("timed_out", None),
        };
        group.kill();
        let exit = match exit {
            Some(exit) => exit,
            None => child.wait().await,
        };
        reaped.send_replace(true);
        (status, exit)
    };
    // Read both streams to EOF even after the cap. Join, rather than try_join, keeps
    // waiting/reaping the process if one output stream fails.
    let (stdout, stderr, (status, exit)) = tokio::join!(
        capture(stdout, &job.stdout, drain.clone()),
        capture(stderr, &job.stderr, drain),
        wait
    );
    let exited_cleanly = matches!(&exit, Ok(exit) if exit.success());
    let mut output = match exit {
        Ok(exit) => format!(
            "exit code: {}",
            exit.code().map_or_else(
                || "none (terminated by a signal)".into(),
                |code| code.to_string()
            )
        ),
        Err(error) => format!("cannot wait for command: {error}"),
    };
    let failed = stdout.is_err() || stderr.is_err() || output.starts_with("cannot wait");
    for (name, captured, result) in [
        ("stdout", &job.stdout, stdout),
        ("stderr", &job.stderr, stderr),
    ] {
        let text = lock(captured).text();
        if !text.is_empty() {
            output.push_str(&format!("\n{name}:\n{text}"));
        }
        if let Err(error) = result {
            output.push_str(&format!("\n{name} read failed: {error}"));
        }
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

async fn capture(
    mut stream: impl AsyncRead + Unpin,
    saved: &Mutex<Capture>,
    mut reaped: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 8192];
    // A child that deliberately leaves the process group can inherit a pipe. It
    // cannot hold this job open forever after its shell has already been reaped.
    let drain_deadline = async move {
        let _ = reaped.wait_for(|value| *value).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    tokio::pin!(drain_deadline);
    loop {
        let count = tokio::select! {
            count = stream.read(&mut buffer) => count?,
            _ = &mut drain_deadline => return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut, "output pipe stayed open after process group termination")),
        };
        if count == 0 {
            return Ok(());
        }
        let mut saved = lock(saved);
        let room = MAX_STREAM_BYTES - saved.bytes.len();
        saved.bytes.extend_from_slice(&buffer[..count.min(room)]);
        saved.truncated |= count > room;
    }
}

impl Capture {
    fn text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.bytes).into_owned();
        let truncated = self.truncated || text.len() > MAX_STREAM_BYTES;
        if text.len() > MAX_STREAM_BYTES {
            let mut end = MAX_STREAM_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        if truncated {
            text.push_str("\n[output truncated]");
        }
        text
    }
}

pub struct Jobs(pub JobOwner);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    job_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Status,
    Output,
    Cancel,
}

impl Tool for Jobs {
    fn name(&self) -> &'static str {
        "jobs"
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Console
    }
    fn description(&self, _: &ToolContext<'_>) -> String {
        "List, inspect output/status, or cancel this thread's background commands. Results are delivered automatically; polling is optional. Output keeps at most 32 KiB per stream. Finished job history is bounded and older IDs expire.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type":"object", "properties": {
            "action": {"type":"string", "enum":["list","status","output","cancel"]},
            "job_id": {"type":"string", "description":"Required except for list."}
        }, "required":["action"], "additionalProperties":false})
    }
    fn call<'a>(&'a self, arguments: Value, _: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let JobOwner { manager, thread_id } = &self.0;
            if matches!(arguments.action, Action::List) {
                ensure!(arguments.job_id.is_none(), "list does not accept job_id");
                let jobs = manager.state.jobs(thread_id).await?;
                if jobs.is_empty() {
                    return Ok("no background jobs".into());
                }
                return Ok(jobs
                    .into_iter()
                    .map(|job| {
                        format!(
                            "{}: {} - {}",
                            job.job_id,
                            manager.status(&job),
                            job.command.chars().take(120).collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            let id = arguments.job_id.context("job_id is required")?;
            if matches!(arguments.action, Action::Cancel) {
                return manager.cancel(thread_id, &id).await;
            }
            let job = manager.state.job(thread_id, &id).await?;
            if matches!(arguments.action, Action::Status) {
                return Ok(format!("{}: {}", job.job_id, manager.status(&job)));
            }
            let status = manager.status(&job);
            let live = manager.live.lock().await;
            let output = match live.get(&id).filter(|job| job.owner == *thread_id) {
                Some(job) => format!(
                    "stdout:\n{}\nstderr:\n{}",
                    lock(&job.stdout).text(),
                    lock(&job.stderr).text()
                ),
                None => job.output,
            };
            Ok(format!("job {id}: {status}\n{output}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::testing::temp_dir;

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

    #[tokio::test]
    async fn a_finished_job_is_announced_once() {
        let (state, database) = open("job-once").await;
        let manager = manager(&state, &database).await;
        let id = manager
            .start("thread", "echo hi", Path::new("."), 10)
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
            .start("thread", "echo bad >&2; exit 3", Path::new("."), 10)
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
            .start("thread", "sleep 30", Path::new("."), 60)
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
        manager(&state, &database).await;
        assert_eq!(
            state.job("thread", "job").await.unwrap().status,
            "interrupted"
        );
        assert_eq!(announced(&state, "job").await, 1);
        assert!(!Path::new(&path).exists());
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
}
