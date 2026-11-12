//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{ProjectRequest, Response, Secret},
    project::{FileDiff, FileEntry, GitInfo, OutputFile, Project, Schedule, ScheduleDraft},
};

use super::*;

pub async fn projects() -> Res<Vec<Project>> {
    ask!(project(ProjectRequest::List), Response::Projects(p) => p)
}

pub async fn save_project(
    id: Option<String>,
    name: String,
    description: String,
    dir: String,
) -> Res<Project> {
    let dir = dir.into();
    ask!(
        project(ProjectRequest::Save { id, name, description, dir }),
        Response::Project(p) => p
    )
}

pub async fn remove_project(id: &str) -> Res<()> {
    done(project(ProjectRequest::Remove { id: id.to_owned() })).await
}

pub async fn git(id: &str) -> Res<GitInfo> {
    ask!(project(ProjectRequest::Git { id: id.to_owned() }), Response::Git(g) => g)
}

pub async fn project_diff(id: &str, path: Option<String>) -> Res<Vec<FileDiff>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::Diff { id, path }), Response::Diff(d) => d)
}

pub async fn switch_branch(id: &str, branch: String) -> Res<()> {
    done(project(ProjectRequest::SwitchBranch {
        id: id.to_owned(),
        branch,
    }))
    .await
}

pub async fn files(id: &str, path: String) -> Res<Vec<FileEntry>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::Files { id, path }), Response::Files(f) => f)
}

pub async fn outputs() -> Res<Vec<OutputFile>> {
    ask!(project(ProjectRequest::Outputs), Response::Outputs(o) => o)
}

pub async fn read_output(path: String) -> Res<String> {
    let path = path.into();
    ask!(project(ProjectRequest::ReadOutput { path }), Response::Text(t) => t)
}

pub async fn delete_output(path: String) -> Res<()> {
    done(project(ProjectRequest::DeleteOutput { path: path.into() })).await
}

pub async fn env_names(id: &str) -> Res<Vec<String>> {
    ask!(project(ProjectRequest::Env { id: id.to_owned() }), Response::Names(n) => n)
}

pub async fn set_env(id: &str, name: String, value: String) -> Res<Vec<String>> {
    let id = id.to_owned();
    let value = Secret(value);
    ask!(project(ProjectRequest::SetEnv { id, name, value }), Response::Names(n) => n)
}

pub async fn remove_env(id: &str, name: String) -> Res<Vec<String>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::RemoveEnv { id, name }), Response::Names(n) => n)
}

pub async fn schedules() -> Res<Vec<Schedule>> {
    ask!(project(ProjectRequest::Schedules), Response::Schedules(s) => s)
}

pub async fn save_schedule(draft: ScheduleDraft) -> Res<Vec<Schedule>> {
    ask!(project(ProjectRequest::SaveSchedule { draft }), Response::Schedules(s) => s)
}

pub async fn delete_schedule(id: String) -> Res<Vec<Schedule>> {
    ask!(project(ProjectRequest::DeleteSchedule { id }), Response::Schedules(s) => s)
}

/// Runs the task now in a new chat; answers with the chat's id.
pub async fn run_schedule(id: String) -> Res<String> {
    ask!(project(ProjectRequest::RunSchedule { id }), Response::Opened { thread_id } => thread_id)
}
