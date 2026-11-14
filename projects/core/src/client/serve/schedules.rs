//! Scheduled tasks: the requests that keep the timetable, and the runs it starts. A run
//! is a new chat in the task's project, sent the task's prompt, with no client attached;
//! it closes when its turn is over.

use super::Server;
use crate::project::{self, schedule};
use anyhow::{Context, Result, ensure};
use chrono::Local;
use dusk_protocol::{
    client::{ProjectRequest, Response},
    project::{MAX_SCHEDULES, Schedule, ScheduleDraft},
    system::now_ms,
};
use std::sync::{Arc, atomic::Ordering};
use tracing::{info, warn};

/// Where the chats a schedule starts say they came from.
const SOURCE: &str = "schedule";

pub(super) fn is_schedule(request: &ProjectRequest) -> bool {
    matches!(
        request,
        ProjectRequest::Schedules
            | ProjectRequest::SaveSchedule { .. }
            | ProjectRequest::DeleteSchedule { .. }
            | ProjectRequest::RunSchedule { .. }
    )
}

impl Server {
    pub(super) async fn schedule(&mut self, request: ProjectRequest) -> Result<Response> {
        let runtime = Arc::clone(&self.runtime);
        let state = runtime.state();
        match request {
            ProjectRequest::SaveSchedule { draft } => self.save_schedule(draft).await?,
            ProjectRequest::DeleteSchedule { id } => state.delete_schedule(&id).await?,
            ProjectRequest::RunSchedule { id } => {
                let task = state
                    .load_schedule(&id)
                    .await?
                    .context("no such scheduled task")?;
                let thread_id = self.start_run(&task).await?;
                state
                    .save_schedule(&Schedule {
                        last_run_at: Some(now_ms()),
                        last_thread_id: Some(thread_id.clone()),
                        ..task
                    })
                    .await?;
                return Ok(Response::Opened { thread_id });
            }
            _ => {}
        }
        Ok(Response::Schedules(state.list_schedules().await?))
    }

    async fn save_schedule(&mut self, draft: ScheduleDraft) -> Result<()> {
        let runtime = Arc::clone(&self.runtime);
        let state = runtime.state();
        let now = now_ms();
        schedule::check(&draft, now)?;
        // Deny by default: a task runs only in a folder the user saved as a project.
        project::find(state, &draft.project_id).await?;
        let old = match &draft.id {
            Some(id) => Some(
                state
                    .load_schedule(id)
                    .await?
                    .context("no such scheduled task")?,
            ),
            None => {
                ensure!(
                    state.count_schedules().await? < MAX_SCHEDULES,
                    "there are already {MAX_SCHEDULES} scheduled tasks"
                );
                None
            }
        };
        let next_run_at = schedule::next_run(&draft.repeat, &Local::now())
            .map(|at| at.timestamp_millis())
            .filter(|_| draft.enabled);
        state
            .save_schedule(&Schedule {
                id: draft.id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                project_id: draft.project_id,
                prompt: draft.prompt.trim().to_owned(),
                repeat: draft.repeat,
                enabled: draft.enabled,
                model: draft.model,
                next_run_at,
                last_run_at: old.as_ref().and_then(|old| old.last_run_at),
                last_thread_id: old.and_then(|old| old.last_thread_id),
            })
            .await
    }

    /// Starts every task whose time has come. A task whose last run is still working is
    /// not started twice; its next time is worked out from now, so a machine that slept
    /// through several runs makes up for them with one.
    pub(super) async fn run_due_schedules(&mut self) {
        let runtime = Arc::clone(&self.runtime);
        let state = runtime.state();
        let due = match state.due_schedules(now_ms()).await {
            Ok(due) => due,
            Err(error) => {
                warn!("cannot read the timetable: {error:#}");
                return;
            }
        };
        for task in due {
            let busy = task
                .last_thread_id
                .as_deref()
                .and_then(|id| self.threads.get(id))
                .is_some_and(|open| !open.done.load(Ordering::Relaxed));
            let mut next = Schedule {
                next_run_at: schedule::next_run(&task.repeat, &Local::now())
                    .map(|at| at.timestamp_millis()),
                ..task.clone()
            };
            if next.next_run_at.is_none() {
                next.enabled = false;
            }
            if busy {
                info!("scheduled task {} is still running; skipped", task.id);
            } else {
                match self.start_run(&task).await {
                    Ok(thread_id) => {
                        next.last_run_at = Some(now_ms());
                        next.last_thread_id = Some(thread_id);
                    }
                    Err(error) => warn!("scheduled task {} did not start: {error:#}", task.id),
                }
            }
            if let Err(error) = state.save_schedule(&next).await {
                warn!("cannot keep the timetable: {error:#}");
            }
        }
    }

    /// A new chat in the task's project, sent the task's prompt, kept running alone.
    async fn start_run(&mut self, task: &Schedule) -> Result<String> {
        let runtime = Arc::clone(&self.runtime);
        let project = project::find(runtime.state(), &task.project_id).await?;
        let handle = match &task.model {
            Some(model) => {
                runtime
                    .new_thread_with_model(SOURCE, &project.dir, model)
                    .await?
            }
            None => runtime.new_thread(SOURCE, &project.dir).await?,
        };
        let thread_id = handle.thread_id.clone();
        self.open(handle);
        let open = self
            .threads
            .get_mut(&thread_id)
            .context("the new chat did not open")?;
        // Opened by the timetable, not by a client.
        open.clients.clear();
        open.unattended = true;
        open.input.send(task.prompt.clone()).await?;
        info!("scheduled task {} started chat {thread_id}", task.id);
        Ok(thread_id)
    }
}
