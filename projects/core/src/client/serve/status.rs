//! The status a client draws from, and the installing of an update.

use super::Server;
use crate::runtime::Runtime;
use anyhow::Result;
use dusk_protocol::{
    client::{Response, Status},
    release::InstallProgress,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{oneshot, watch};

pub(super) fn read_status(runtime: &Runtime, install: Option<InstallProgress>) -> Status {
    let config = runtime.config().current();
    Status {
        model: runtime.model().map(|choice| choice.name()),
        discovering_model: runtime.discovering_model(),
        input_limit: runtime.input_limit(),
        context_policy: runtime.context_policy(),
        effort: config.effort(),
        mode: config.mode().to_owned(),
        approvals: config.approvals(),
        max_goal_rounds: config.limits().max_goal_rounds,
        idle_screen_minutes: config.appearance().idle_screen_minutes,
        notifications: config.notifications(),
        sound_notifications: config.sound_notifications(),
        // Read before the statuses: a server that settles in between still shows.
        mcp_starting: runtime.mcp_starting(),
        mcp: runtime.mcp_statuses(),
        plugins: runtime.plugins(),
        available_update: runtime.available_update(),
        install,
        updated_builtin: runtime.updated_builtin().to_vec(),
        chatgpt: runtime.chatgpt(),
        limits: runtime.model().and_then(|choice| {
            let settings = config.providers().get(&choice.provider_name)?;
            crate::providers::Account::of(settings).limits()
        }),
        artifacts_dir: runtime.paths().artifacts_dir(),
        workspace_dir: runtime.paths().workspace_root().to_path_buf(),
    }
}

impl Server {
    /// Installs a release in the background; the reply comes once it is done, and the
    /// status shows how far it got meanwhile.
    pub(super) fn install_update(&self, tag: String, reply: oneshot::Sender<Response>) {
        let runtime = Arc::clone(&self.runtime);
        let status = Arc::clone(&self.status);
        status.send_modify(|status| status.install = Some(InstallProgress::default()));
        tokio::spawn(async move {
            let (sender, mut progress) = watch::channel(InstallProgress::default());
            let install = runtime.install_update_with_progress(&tag, sender);
            tokio::pin!(install);
            let result: Result<PathBuf> = loop {
                tokio::select! {
                    result = &mut install => break result,
                    changed = progress.changed() => {
                        if changed.is_ok() {
                            let now = *progress.borrow_and_update();
                            status.send_modify(|status| status.install = Some(now));
                        }
                    }
                }
            };
            let now = *progress.borrow();
            status.send_modify(|status| status.install = result.is_ok().then_some(now));
            let response = match result {
                Ok(path) => Response::Path(path),
                Err(error) => Response::Error(format!("{error:#}")),
            };
            let _ = reply.send(response);
        });
    }

    pub(super) fn refresh_status(&self) {
        let mut status = read_status(&self.runtime, None);
        self.status.send_if_modified(|old| {
            // The install task writes its own field; it is never read here.
            status.install = old.install;
            let changed = *old != status;
            if changed {
                *old = status;
            }
            changed
        });
    }
}
