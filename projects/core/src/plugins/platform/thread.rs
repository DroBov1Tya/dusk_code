//! The thread a platform talks to: its newest one, or a new one.

use anyhow::Result;

use crate::runtime::Launcher;

/// What a platform's threads are recorded as having come from.
fn source(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

/// The platform's newest thread, other than one that just ended, or a new one.
pub(super) async fn open_thread(
    launcher: &Launcher,
    plugin: &str,
    ended: Option<&str>,
) -> Result<crate::runtime::ThreadHandle> {
    let source = source(plugin);
    let newest = launcher
        .state
        .list_threads()
        .await?
        .into_iter()
        .find(|thread| thread.source == source && Some(thread.id.as_str()) != ended);
    match newest {
        Some(thread) => {
            launcher
                .resume_thread(&thread.id, launcher.paths.workspace_root())
                .await
        }
        None => new_thread(launcher, plugin).await,
    }
}

pub(super) async fn new_thread(
    launcher: &Launcher,
    plugin: &str,
) -> Result<crate::runtime::ThreadHandle> {
    let cwd = launcher.paths.workspace_root().to_owned();
    crate::init::create_private_dir(&cwd)?;
    launcher.new_thread(&source(plugin), &cwd).await
}
