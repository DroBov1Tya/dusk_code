//! Where the thread's tools act: its own folder, or the worktree of the branch it works
//! on (`tools::git`). The record decides, read before each step, so a branch started by
//! a call holds from the next step and a restarted thread is back in its worktree.

use std::sync::Arc;

use super::Agent;
use crate::tools::ToolScope;

impl Agent {
    /// The scope the thread's tools act in now.
    pub fn working_scope(&self) -> Arc<ToolScope> {
        Arc::clone(&self.here.lock())
    }

    /// Moves the tools to the thread's worktree, or back to its folder.
    pub(super) async fn follow_worktree(&self) {
        let now = self.working_scope();
        let next =
            crate::tools::worktree_scope(&self.state, &self.thread_id, &self.scope, &now).await;
        if !Arc::ptr_eq(&now, &next) {
            *self.here.lock() = next;
        }
    }
}
