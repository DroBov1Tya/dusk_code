//! Changing a project's repository.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What to do in a saved project's repository. Paths are relative to the project folder;
/// branch names are letters, digits and `-_./`. Nothing here forces, amends or prunes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRequest {
    /// A new branch from `base`, or from the current commit. With `switch` the work tree
    /// moves to it, its changes carried along; git refuses when they would be lost.
    CreateBranch {
        name: String,
        base: Option<String>,
        #[serde(default)]
        switch: bool,
    },
    Stage {
        paths: Vec<String>,
    },
    StageAll,
    Unstage {
        paths: Vec<String>,
    },
    /// Commits what is staged; never amends.
    Commit {
        message: String,
    },
    /// Destructive: the file's uncommitted changes, or the untracked file itself, are
    /// gone. A client asks the user first.
    Discard {
        path: String,
    },
    /// Merges `branch` into the current one; conflicts are reported, not resolved.
    Merge {
        branch: String,
    },
    AbortMerge,
    /// A worktree for `branch` beside the repository, in `<repository>-worktrees/`; the
    /// branch is made from `base` (or the current commit) when it does not exist.
    AddWorktree {
        branch: String,
        base: Option<String>,
    },
    /// Refused while the worktree has uncommitted or untracked files.
    RemoveWorktree {
        path: PathBuf,
    },
    Fetch,
    /// Fast-forward only.
    Pull,
    /// The current branch to its upstream, or to `origin` as its new upstream; never
    /// forced. A client asks the user first.
    Push,
}
