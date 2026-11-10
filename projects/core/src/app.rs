//! What a client's process takes from the core besides the protocol: its folders and
//! logs, the start of the core, the commands that run without a thread such as an
//! update or a restore, and the few helpers that work on the client's own machine.
//! Nothing here reaches a running thread; that goes through `client`.

use std::path::PathBuf;

use anyhow::Result;
use dusk_protocol::client::Hello;

#[cfg(target_os = "macos")]
pub use crate::tools::run_overlay_if_asked;
pub use crate::{
    backup::archive::{RemovalGuard, RestoreGuard, recover, resolve_state_db, restore},
    catalog::mentions::complete as complete_mention,
    init::{
        logs::{Telemetry, init as init_logs},
        paths::Paths,
    },
    message::path_completion,
    release::{
        complete::{Step, complete, ensure_installed},
        resources::{data_root, install_snapshot},
        update::{VERSION, update_now},
    },
    state::artifacts::ArtifactStore,
    util::usage::process as process_usage,
};
use crate::{
    client::{self, Client},
    runtime::Runtime,
};

/// Starts the core over `paths` and serves the client that said `hello`.
pub async fn start(paths: Paths, state_db: Option<PathBuf>, hello: Hello) -> Result<Client> {
    let runtime = Runtime::start(paths, state_db).await?;
    client::serve(runtime, hello)
}
