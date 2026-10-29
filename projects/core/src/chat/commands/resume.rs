//! `/resume`: the threads a client can open again, the most recently active first.

use anyhow::Result;

use crate::{runtime::Runtime, state::Thread};

pub async fn run(runtime: &Runtime) -> Result<Vec<Thread>> {
    runtime.list_threads().await
}
