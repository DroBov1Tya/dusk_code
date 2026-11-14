//! The platforms of plugins that are on, each kept running: one that ends is started
//! again after a wait.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use parking_lot::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::{plugins::Isolation, runtime::Launcher};

use super::session::Session;

/// Waits before a platform that ended is started again, doubled up to the last.
const RESTART_WAITS: [u64; 4] = [2, 10, 30, 60];

/// A platform of a plugin that is on.
#[derive(Clone)]
pub struct PlatformSpec {
    pub plugin: String,
    pub command: Vec<String>,
    pub isolation: Isolation,
}

#[derive(Default)]
pub struct Platforms {
    launcher: std::sync::OnceLock<Launcher>,
    /// Each running platform with what it was started from, to tell a change.
    running: Mutex<BTreeMap<String, (String, JoinHandle<()>)>>,
    statuses: Arc<RwLock<BTreeMap<String, String>>>,
}

impl Platforms {
    /// Gives the platforms the core to start threads with; set once at start.
    pub fn attach(&self, launcher: Launcher) {
        let _ = self.launcher.set(launcher);
    }

    pub fn status(&self, plugin: &str) -> Option<String> {
        self.statuses.read().get(plugin).cloned()
    }

    /// Starts the platforms that are new or changed and stops those that are gone.
    pub fn set(&self, specs: Vec<PlatformSpec>) {
        let Some(launcher) = self.launcher.get() else {
            return;
        };
        let wanted: BTreeMap<String, (String, PlatformSpec)> = specs
            .into_iter()
            .map(|spec| {
                let started_from = format!("{:?} {}", spec.command, spec.isolation.fingerprint());
                (spec.plugin.clone(), (started_from, spec))
            })
            .collect();
        let mut running = self.running.lock();
        running.retain(|name, (started_from, task)| {
            let keep = wanted
                .get(name)
                .is_some_and(|(wanted, _)| wanted == started_from);
            if !keep {
                task.abort();
                self.statuses.write().remove(name);
                info!("platform of plugin {name} stops");
            }
            keep
        });
        for (name, (started_from, spec)) in wanted {
            if running.contains_key(&name) {
                continue;
            }
            info!("platform of plugin {name} starts");
            let task = tokio::spawn(keep_running(
                launcher.clone(),
                spec,
                Arc::clone(&self.statuses),
            ));
            running.insert(name, (started_from, task));
        }
    }

    pub fn stop_all(&self) {
        for (_, (_, task)) in std::mem::take(&mut *self.running.lock()) {
            task.abort();
        }
    }
}

async fn keep_running(
    launcher: Launcher,
    spec: PlatformSpec,
    statuses: Arc<RwLock<BTreeMap<String, String>>>,
) {
    let mut attempt = 0usize;
    loop {
        let ended = match Session::run(&launcher, &spec, &statuses).await {
            Ok(()) => "the platform ended".to_owned(),
            Err(error) => format!("the platform failed: {error:#}"),
        };
        warn!("plugin {}: {ended}", spec.plugin);
        statuses.write().insert(spec.plugin.clone(), ended);
        let wait = RESTART_WAITS[attempt.min(RESTART_WAITS.len() - 1)];
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}
