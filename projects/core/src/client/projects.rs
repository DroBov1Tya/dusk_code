//! Projects, and the git state and files of one.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{ProjectRequest, Request, Response, Secret},
    project::{
        FileDiff, FileEntry, FileStamp, FileText, GitInfo, OutputFile, Project, Schedule,
        ScheduleDraft,
    },
};
use std::path::PathBuf;

pub struct Projects<'a>(pub(super) &'a Client);

impl Projects<'_> {
    pub async fn list(&self) -> Result<Vec<Project>> {
        match self.0.call(Request::Project(ProjectRequest::List)).await? {
            Response::Projects(projects) => Ok(projects),
            other => Err(unexpected(other)),
        }
    }

    /// Saves a new project when `id` is `None`, or changes that one.
    pub async fn save(
        &self,
        id: Option<String>,
        name: String,
        description: String,
        dir: PathBuf,
    ) -> Result<Project> {
        let request = ProjectRequest::Save {
            id,
            name,
            description,
            dir,
        };
        match self.0.call(Request::Project(request)).await? {
            Response::Project(project) => Ok(project),
            other => Err(unexpected(other)),
        }
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.0
            .done(Request::Project(ProjectRequest::Remove { id }))
            .await
    }

    /// The names of the project's environment variables.
    pub async fn env(&self, id: &str) -> Result<Vec<String>> {
        let id = id.to_owned();
        self.names(ProjectRequest::Env { id }).await
    }

    /// Sets a variable for the commands that run in the project; answers with the names.
    pub async fn set_env(&self, id: &str, name: &str, value: &str) -> Result<Vec<String>> {
        let (id, name) = (id.to_owned(), name.to_owned());
        let value = Secret(value.to_owned());
        self.names(ProjectRequest::SetEnv { id, name, value }).await
    }

    pub async fn remove_env(&self, id: &str, name: &str) -> Result<Vec<String>> {
        let (id, name) = (id.to_owned(), name.to_owned());
        self.names(ProjectRequest::RemoveEnv { id, name }).await
    }

    async fn names(&self, request: ProjectRequest) -> Result<Vec<String>> {
        match self.0.call(Request::Project(request)).await? {
            Response::Names(names) => Ok(names),
            other => Err(unexpected(other)),
        }
    }

    pub async fn git(&self, id: &str) -> Result<GitInfo> {
        let id = id.to_owned();
        match self
            .0
            .call(Request::Project(ProjectRequest::Git { id }))
            .await?
        {
            Response::Git(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    pub async fn switch_branch(&self, id: &str, branch: &str) -> Result<()> {
        let (id, branch) = (id.to_owned(), branch.to_owned());
        let request = ProjectRequest::SwitchBranch { id, branch };
        self.0.done(Request::Project(request)).await
    }

    /// The project's changes file by file, or one file's.
    pub async fn diff(&self, id: &str, path: Option<&str>) -> Result<Vec<FileDiff>> {
        let (id, path) = (id.to_owned(), path.map(str::to_owned));
        match self
            .0
            .call(Request::Project(ProjectRequest::Diff { id, path }))
            .await?
        {
            Response::Diff(files) => Ok(files),
            other => Err(unexpected(other)),
        }
    }

    /// The entries of `path`, a folder relative to the project's, `""` for its top.
    pub async fn files(&self, id: &str, path: &str) -> Result<Vec<FileEntry>> {
        let (id, path) = (id.to_owned(), path.to_owned());
        let request = ProjectRequest::Files { id, path };
        match self.0.call(Request::Project(request)).await? {
            Response::Files(files) => Ok(files),
            other => Err(unexpected(other)),
        }
    }
}

impl Projects<'_> {
    pub async fn outputs(&self) -> Result<Vec<OutputFile>> {
        match self
            .0
            .call(Request::Project(ProjectRequest::Outputs))
            .await?
        {
            Response::Outputs(files) => Ok(files),
            other => Err(unexpected(other)),
        }
    }

    pub async fn delete_output(&self, path: PathBuf) -> Result<()> {
        self.0
            .done(Request::Project(ProjectRequest::DeleteOutput { path }))
            .await
    }

    pub async fn read_output(&self, path: PathBuf) -> Result<String> {
        self.0
            .text(Request::Project(ProjectRequest::ReadOutput { path }))
            .await
    }

    /// A text file for the editor, with the stamp a later `write_file` sends back.
    pub async fn read_file(&self, path: PathBuf) -> Result<FileText> {
        match self
            .0
            .call(Request::Project(ProjectRequest::ReadFile { path }))
            .await?
        {
            Response::File(file) => Ok(file),
            other => Err(unexpected(other)),
        }
    }

    /// Writes the whole file and answers with its new stamp. Refused when `seen` is given
    /// and the file on disk no longer has that stamp.
    pub async fn write_file(
        &self,
        path: PathBuf,
        text: String,
        seen: Option<FileStamp>,
    ) -> Result<FileStamp> {
        let request = ProjectRequest::WriteFile { path, text, seen };
        match self.0.call(Request::Project(request)).await? {
            Response::Stamp(stamp) => Ok(stamp),
            other => Err(unexpected(other)),
        }
    }

    /// Every scheduled task, soonest first.
    pub async fn schedules(&self) -> Result<Vec<Schedule>> {
        self.timetable(ProjectRequest::Schedules).await
    }

    pub async fn save_schedule(&self, draft: ScheduleDraft) -> Result<Vec<Schedule>> {
        self.timetable(ProjectRequest::SaveSchedule { draft }).await
    }

    pub async fn delete_schedule(&self, id: String) -> Result<Vec<Schedule>> {
        self.timetable(ProjectRequest::DeleteSchedule { id }).await
    }

    /// Runs the task now in a new chat; answers with the chat's id.
    pub async fn run_schedule(&self, id: String) -> Result<String> {
        match self
            .0
            .call(Request::Project(ProjectRequest::RunSchedule { id }))
            .await?
        {
            Response::Opened { thread_id } => Ok(thread_id),
            other => Err(unexpected(other)),
        }
    }

    async fn timetable(&self, request: ProjectRequest) -> Result<Vec<Schedule>> {
        match self.0.call(Request::Project(request)).await? {
            Response::Schedules(schedules) => Ok(schedules),
            other => Err(unexpected(other)),
        }
    }
}
