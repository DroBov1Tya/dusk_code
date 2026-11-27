//! A thread's jobs, sources, plan, team, changes and Overview.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, WorkRequest},
    project::{Change, FileChange, FileDiff},
    thread::{JobInfo, LogEntry, Overview, PlanFile, PlanPage, SourceChunk, Team},
};

/// A thread's background jobs and sources.
pub struct Work<'a>(pub(super) &'a Client);

impl Work<'_> {
    pub async fn jobs(&self, thread_id: &str) -> Result<Vec<JobInfo>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Jobs { thread_id }))
            .await?
        {
            Response::Jobs(jobs) => Ok(jobs),
            other => Err(unexpected(other)),
        }
    }

    pub async fn plan(&self, thread_id: &str) -> Result<Option<String>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Plan { thread_id }))
            .await?
        {
            Response::Plan(plan) => Ok(plan),
            other => Err(unexpected(other)),
        }
    }

    /// The changes in the folder the chat started in, file by file, or one file's.
    pub async fn diff(&self, thread_id: &str, path: Option<&str>) -> Result<Vec<FileDiff>> {
        let (thread_id, path) = (thread_id.to_owned(), path.map(str::to_owned));
        match self
            .0
            .call(Request::Work(WorkRequest::Diff { thread_id, path }))
            .await?
        {
            Response::Diff(files) => Ok(files),
            other => Err(unexpected(other)),
        }
    }

    /// The files the chat's commands changed in its project, by path.
    pub async fn changed_files(&self, thread_id: &str) -> Result<Vec<Change>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::ChangedFiles { thread_id }))
            .await?
        {
            Response::Changes(changes) => Ok(changes),
            other => Err(unexpected(other)),
        }
    }

    /// What the chat's commands changed in its project, newest first, with diffs.
    pub async fn file_changes(&self, thread_id: &str) -> Result<Vec<FileChange>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::FileChanges { thread_id }))
            .await?
        {
            Response::FileChanges(changes) => Ok(changes),
            other => Err(unexpected(other)),
        }
    }

    /// How the thread's work stands: activity, changes and the newest log lines.
    pub async fn overview(&self, thread_id: &str) -> Result<Overview> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Overview { thread_id }))
            .await?
        {
            Response::Overview(overview) => Ok(overview),
            other => Err(unexpected(other)),
        }
    }

    /// A page of the thread's event log older than `before`, newest first.
    pub async fn events(
        &self,
        thread_id: &str,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<LogEntry>> {
        let thread_id = thread_id.to_owned();
        let request = WorkRequest::Events {
            thread_id,
            before,
            limit,
        };
        match self.0.call(Request::Work(request)).await? {
            Response::Events(entries) => Ok(entries),
            other => Err(unexpected(other)),
        }
    }

    /// One page of the plan: every task not done, then up to `limit` done ones.
    pub async fn plan_tasks(&self, thread_id: &str, limit: u32) -> Result<PlanPage> {
        let thread_id = thread_id.to_owned();
        let request = WorkRequest::PlanTasks {
            thread_id,
            done_before: None,
            limit,
        };
        match self.0.call(Request::Work(request)).await? {
            Response::PlanPage(page) => Ok(page),
            other => Err(unexpected(other)),
        }
    }

    /// The team running the chat's plan, or none before one ran.
    pub async fn team(&self, thread_id: &str) -> Result<Option<Team>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Team { thread_id }))
            .await?
        {
            Response::Team(team) => Ok(team),
            other => Err(unexpected(other)),
        }
    }

    /// Passes a task of the plan that waits for the user's confirmation.
    pub async fn confirm_task(&self, thread_id: &str, task_id: &str) -> Result<()> {
        let (thread_id, task_id) = (thread_id.to_owned(), task_id.to_owned());
        self.0
            .done(Request::Work(WorkRequest::ConfirmTask {
                thread_id,
                task_id,
            }))
            .await
    }

    /// Retries a failed task of the chat's plan in place.
    pub async fn retry_task(&self, thread_id: &str, task_id: &str) -> Result<()> {
        let (thread_id, task_id) = (thread_id.to_owned(), task_id.to_owned());
        self.0
            .done(Request::Work(WorkRequest::RetryTask { thread_id, task_id }))
            .await
    }

    /// Ends the chat's running team as stopped: its members at work are cancelled and
    /// their tasks go back to open.
    pub async fn stop_team(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.0
            .done(Request::Work(WorkRequest::StopTeam { thread_id }))
            .await
    }

    /// Sets how many members of the chat's running team work at once, 1 to the setting's.
    pub async fn set_team_at_once(&self, thread_id: &str, at_once: u32) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.0
            .done(Request::Work(WorkRequest::SetTeamAtOnce {
                thread_id,
                at_once,
            }))
            .await
    }

    /// Every plan of the chat, newest written first.
    pub async fn plans(&self, thread_id: &str) -> Result<Vec<PlanFile>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Plans { thread_id }))
            .await?
        {
            Response::Plans(plans) => Ok(plans),
            other => Err(unexpected(other)),
        }
    }

    /// Deletes the plans a newer one replaced and finished ones; says how many.
    pub async fn remove_old_plans(&self, thread_id: &str) -> Result<u64> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::RemoveOldPlans { thread_id }))
            .await?
        {
            Response::Number(removed) => Ok(removed),
            other => Err(unexpected(other)),
        }
    }

    /// Rewrites the plan; empty text removes it.
    pub async fn save_plan(&self, thread_id: &str, text: &str) -> Result<()> {
        let (thread_id, text) = (thread_id.to_owned(), text.to_owned());
        self.0
            .done(Request::Work(WorkRequest::SavePlan { thread_id, text }))
            .await
    }

    pub async fn job_output(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
        let request = Request::Work(WorkRequest::JobOutput { thread_id, job_id });
        self.0.text(request).await
    }

    pub async fn stop_job(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
        let request = Request::Work(WorkRequest::StopJob { thread_id, job_id });
        self.0.text(request).await
    }

    pub async fn pin_source(&self, thread_id: &str, id: &str, pinned: bool) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        let request = WorkRequest::PinSource {
            thread_id,
            id,
            pinned,
        };
        self.0.done(Request::Work(request)).await
    }

    /// The bytes of the thread's picture source `source_id`, or else the newest named
    /// `name`.
    pub async fn source_picture(
        &self,
        thread_id: &str,
        name: &str,
        source_id: Option<&str>,
    ) -> Result<Vec<u8>> {
        let request = WorkRequest::SourcePicture {
            thread_id: thread_id.to_owned(),
            name: name.to_owned(),
            source_id: source_id.map(str::to_owned),
        };
        match self.0.call(Request::Work(request)).await? {
            Response::Picture(bytes) => Ok(bytes),
            other => Err(unexpected(other)),
        }
    }

    /// A part of the source `source_id` from byte `offset`, for a reader.
    pub async fn source_text(
        &self,
        thread_id: &str,
        source_id: &str,
        offset: u64,
    ) -> Result<SourceChunk> {
        let (thread_id, source_id) = (thread_id.to_owned(), source_id.to_owned());
        let request = WorkRequest::SourceText {
            thread_id,
            source_id,
            offset,
        };
        match self.0.call(Request::Work(request)).await? {
            Response::SourceText(chunk) => Ok(chunk),
            other => Err(unexpected(other)),
        }
    }

    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        let request = WorkRequest::RemoveSource { thread_id, id };
        self.0.done(Request::Work(request)).await
    }
}
