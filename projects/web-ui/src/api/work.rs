//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{Response, WorkRequest},
    project::{Change, FileDiff},
    thread::JobInfo,
};

use super::*;

pub async fn jobs(thread_id: &str) -> Res<Vec<JobInfo>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Jobs { thread_id }), Response::Jobs(j) => j)
}

pub async fn job_output(thread_id: &str, job_id: &str) -> Res<String> {
    let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
    ask!(work(WorkRequest::JobOutput { thread_id, job_id }), Response::Text(t) => t)
}

pub async fn stop_job(thread_id: &str, job_id: &str) -> Res<String> {
    let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
    ask!(work(WorkRequest::StopJob { thread_id, job_id }), Response::Text(t) => t)
}

pub async fn plan(thread_id: &str) -> Res<Option<String>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Plan { thread_id }), Response::Plan(p) => p)
}

pub async fn save_plan(thread_id: &str, text: String) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(work(WorkRequest::SavePlan { thread_id, text })).await
}

pub async fn chat_diff(thread_id: &str, path: Option<String>) -> Res<Vec<FileDiff>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Diff { thread_id, path }), Response::Diff(d) => d)
}

pub async fn changed_files(thread_id: &str) -> Res<Vec<Change>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::ChangedFiles { thread_id }), Response::Changes(c) => c)
}
