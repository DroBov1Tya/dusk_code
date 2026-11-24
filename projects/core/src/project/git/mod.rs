//! A project's repository through the git program: an argument vector and never a
//! shell, `--end-of-options` before names and `--` before paths, no prompts, no editor,
//! and a time limit. Branch names and paths are checked before git sees them (`check`).
//! Nothing here forces a push, amends a commit or prunes a worktree.

mod branches;
mod check;
mod read;
mod remote;
mod run;
#[cfg(test)]
mod tests;
mod worktree;
mod write;

pub use branches::{
    Repository, apart, files_between, merging, names, remove_merged, repository, unsaved,
};
pub use read::{changes, info, summary};
pub use remote::{fetch, pull, push};
pub use worktree::{add as add_worktree, remove as remove_worktree};
pub use write::{
    abort_merge, commit, create_branch, discard, merge, merge_recorded, stage, stage_all,
    switch_branch, unstage,
};

pub(crate) use check::{branch as check_branch, message as check_message};
pub(super) use read::web_page;
pub(super) use run::{Output, command, finish, git, git_differs};
