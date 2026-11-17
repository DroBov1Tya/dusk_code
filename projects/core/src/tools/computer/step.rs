//! One checked screen action: what it asks the user and the work it does. A tool alone
//! and a batch build the same step, so they cannot ask or act differently.

use anyhow::Result;

use super::blocking;
use crate::tools::{Request, ToolResult, tool::ToolContext};

type Work = Box<dyn FnOnce() -> Result<ToolResult> + Send>;

pub(super) struct Step {
    /// What the user is asked before the work runs.
    pub request: Request,
    work: Work,
}

impl Step {
    pub fn new(
        request: Request,
        work: impl FnOnce() -> Result<ToolResult> + Send + 'static,
    ) -> Self {
        Self {
            request,
            work: Box::new(work),
        }
    }

    /// The work alone; the caller has asked for it.
    pub async fn run(self) -> Result<ToolResult> {
        blocking(self.work).await
    }

    /// A tool called on its own: asks, then acts.
    pub async fn ask_and_run(self, context: &ToolContext) -> Result<ToolResult> {
        context.approve_computer(vec![self.request.clone()]).await?;
        self.run().await
    }
}
