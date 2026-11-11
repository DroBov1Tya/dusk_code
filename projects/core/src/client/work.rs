//! A thread's jobs, sources, plan and changes.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, WorkRequest},
    project::{Change, FileDiff},
    thread::JobInfo,
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

    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        let request = WorkRequest::RemoveSource { thread_id, id };
        self.0.done(Request::Work(request)).await
    }
}
