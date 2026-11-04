//! What the computer tools share: how they are offered and how they leave the
//! async runtime.

use anyhow::{Context, Result};

/// What the tools share: how they are offered.
macro_rules! computer_tool {
    ($name:literal) => {
        fn name(&self) -> &str {
            $name
        }

        fn kind(&self) -> ToolKind {
            ToolKind::Computer
        }

        fn needs(&self) -> Needs {
            Needs::Gui
        }

        /// One desktop and shared keyboard: the chat drives them, never its agents.
        fn root_only(&self) -> bool {
            true
        }
    };
}

pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .context("the screen action stopped")?
}
