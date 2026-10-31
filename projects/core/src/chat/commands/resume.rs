//! `/resume`: the threads a client can open again, the most recently active first: those
//! started in the folder it runs in, or all of them when it names none.

use std::path::PathBuf;

use anyhow::Result;

use crate::{runtime::Runtime, state::Thread};

pub async fn run(runtime: &Runtime, cwd: Option<PathBuf>) -> Result<Vec<Thread>> {
    match cwd {
        Some(cwd) => runtime.project_threads(&cwd).await,
        None => runtime.list_threads().await,
    }
}
