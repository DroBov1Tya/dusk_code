//! Projects, and the git state and files of one.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::client::{GitRequest, Secret};
use crate::project::{FileStamp, ScheduleDraft};
/// Projects, and the git state and files of one. Git and files are read only inside
/// a saved project's folder.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectRequest {
    List,
    /// Saves a new project, or changes the one with `id`; answers with the project.
    Save {
        id: Option<String>,
        name: String,
        description: String,
        dir: PathBuf,
    },
    Remove {
        id: String,
    },
    Git {
        id: String,
    },
    /// The changes against the last commit, file by file with their hunks; one file
    /// when `path` is given. Untracked files show as all added.
    Diff {
        id: String,
        path: Option<String>,
    },
    /// Refused while the work tree has changes, so nothing is lost.
    SwitchBranch {
        id: String,
        branch: String,
    },
    /// Changes the repository. Answered beside the loop, since a push may take a
    /// while; a change is followed by
    /// `Event::GitChanged`.
    Repository {
        id: String,
        request: GitRequest,
    },
    /// The work tree's changes and the pull requests of the repository, answered beside
    /// the loop. The asking client then watches the project, in place of the one it
    /// watched before: while it stays attached the core reads it again, often while
    /// checks run and seldom otherwise, and sends `Event::RepositoryStatus` on a change.
    RepositoryStatus {
        id: String,
    },
    /// One pull request of the project's own forge repository, for a card where a chat
    /// links it; answered beside the loop from what the core knows or read lately, and
    /// read from the forge at most once at a time and every few minutes per pull request.
    PullStatus {
        id: String,
        number: u64,
    },
    /// A pull request from the project's current branch (never a detached head), which
    /// must be pushed first, into `base` or the repository's default branch; answered
    /// beside the loop with `Response::PullStatus(Some(_))` as the forge now has it.
    CreatePull {
        id: String,
        title: String,
        body: String,
        base: Option<String>,
    },
    /// The pull requests of each saved project's current branch, from what the core
    /// last read; a project not read yet is read in the background.
    PullSummaries,
    /// The entries of `path`, a folder relative to the project's, `""` for its top.
    Files {
        id: String,
        path: String,
    },
    /// The files under the workspace and the pictures folder, newest first.
    Outputs,
    /// The names of the project's environment variables. Values are never sent back.
    Env {
        id: String,
    },
    /// Sets one variable for the project's commands; answers with the names.
    SetEnv {
        id: String,
        name: String,
        value: Secret,
    },
    /// Removes one; answers with the names left.
    RemoveEnv {
        id: String,
        name: String,
    },
    /// A text file for a preview, read only under the workspace, the pictures folder or
    /// a saved project, and only up to `MAX_PREVIEW_BYTES`.
    ReadOutput {
        path: PathBuf,
    },
    /// A text file for the editor, read where `ReadOutput` reads and up to
    /// `MAX_EDIT_BYTES`; answers with the text and the file's stamp.
    ReadFile {
        path: PathBuf,
    },
    /// Writes the whole file, only where `ReadFile` reads; a new file is made when its
    /// folder is there. `seen` is the stamp the client read with: the write is refused
    /// when the file on disk no longer has it. Answers with the new stamp.
    WriteFile {
        path: PathBuf,
        text: String,
        seen: Option<FileStamp>,
    },
    /// Removes a file under the workspace or the pictures folder, never a project's.
    DeleteOutput {
        path: PathBuf,
    },
    /// Every scheduled task, soonest first.
    Schedules,
    /// Saves a new scheduled task, or changes the one with the draft's id; answers with
    /// every scheduled task.
    SaveSchedule {
        draft: ScheduleDraft,
    },
    DeleteSchedule {
        id: String,
    },
    /// Runs it now, in a new chat, without changing when it runs next; answers with
    /// the chat's id.
    RunSchedule {
        id: String,
    },
}
