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
    backup::{
        Uninstall,
        archive::{RestoreGuard, recover, resolve_state_db, restore},
    },
    catalog::mentions::complete as complete_mention,
    init::{
        logs::{Telemetry, init as init_logs},
        paths::{Paths, expand_home},
    },
    message::path_completion,
    platform::{notify, usage::process as process_usage},
    project::git_summary,
    release::{
        app_update::running_bundle,
        complete::{Step, complete, ensure_installed},
        notes::{Notes, notes_since},
        resources::install_snapshot,
        update::{VERSION, update_now},
    },
    state::artifacts::ArtifactStore,
};
use crate::{
    client::{self, Client, Hub},
    runtime::Runtime,
};
use std::sync::Arc;

/// Starts the core over `paths` and serves the client that said `hello`.
pub async fn start(paths: Paths, state_db: Option<PathBuf>, hello: Hello) -> Result<Client> {
    let runtime = Runtime::start(paths, state_db).await?;
    client::serve(runtime, hello)
}

/// Starts the core over `paths` for several clients: each one attaches to the hub.
pub async fn start_hub(paths: Paths, state_db: Option<PathBuf>) -> Result<Arc<Hub>> {
    Ok(Hub::start(Runtime::start(paths, state_db).await?))
}
