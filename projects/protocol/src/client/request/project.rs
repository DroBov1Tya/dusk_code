//! Projects, and the git state and files of one.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::client::Secret;
use crate::project::ScheduleDraft;
/// Projects, and the git state and files of one. Git and files are read only inside
/// a saved project's folder.
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
