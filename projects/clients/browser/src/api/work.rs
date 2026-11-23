//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{Response, WorkRequest},
    project::{Change, FileDiff},
    thread::{JobInfo, LogEntry, Overview, PlanCounts, Team},
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

/// Keeps a picture among the thread's sources; answers with the tag for the message.
pub async fn upload_picture(thread_id: &str, name: String, bytes: Vec<u8>) -> Res<String> {
    let thread_id = thread_id.to_owned();
    ask!(
        work(WorkRequest::UploadPicture { thread_id, name, bytes }),
        Response::Text(tag) => tag
    )
}

/// How the thread's work stands: activity, changes and the newest log lines.
pub async fn overview(thread_id: &str) -> Res<Overview> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Overview { thread_id }), Response::Overview(o) => o)
}

/// A page of the thread's event log older than `before`, newest first.
pub async fn events(thread_id: &str, before: Option<i64>, limit: u32) -> Res<Vec<LogEntry>> {
    let thread_id = thread_id.to_owned();
    ask!(
        work(WorkRequest::Events { thread_id, before, limit }),
        Response::Events(entries) => entries
    )
}

/// How many tasks of the chat's plan are in each status.
pub async fn plan_counts(thread_id: &str) -> Res<PlanCounts> {
    let thread_id = thread_id.to_owned();
    ask!(
        work(WorkRequest::PlanTasks { thread_id, done_before: None, limit: 1 }),
        Response::PlanPage(page) => page.counts
    )
}

/// The team running the chat's plan, or none before one ran.
pub async fn team(thread_id: &str) -> Res<Option<Team>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Team { thread_id }), Response::Team(t) => t)
}

/// Ends the chat's running team as stopped; its events move the plan and the team.
pub async fn stop_team(thread_id: &str) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(work(WorkRequest::StopTeam { thread_id })).await
}

/// Sets how many members of the chat's running team work at once, 1 to the setting's.
pub async fn set_team_at_once(thread_id: &str, at_once: u32) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(work(WorkRequest::SetTeamAtOnce { thread_id, at_once })).await
}

/// Passes a task that waits for the user's confirmation; its own event moves the row.
pub async fn confirm_task(thread_id: &str, task_id: &str) -> Res<()> {
    let (thread_id, task_id) = (thread_id.to_owned(), task_id.to_owned());
    done(work(WorkRequest::ConfirmTask { thread_id, task_id })).await
}
