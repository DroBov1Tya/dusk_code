//! The interfaces of plugins that are on: which run, and which client opened which part.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::{
    client::Event,
    plugin::{MAX_TEXT, Part, ToPlugin, UiEvent},
};
use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;
use tracing::info;

use super::process::{self, Owners};
use crate::plugins::isolation::Isolation;

/// Who is attached to the core; the same numbers as the server's.
pub type ClientId = u64;
/// The longest widget id an event may name.
const MAX_WIDGET_ID: usize = 100;

/// The interface of a plugin that is on.
pub struct UiSpec {
    pub plugin: String,
    /// The program and its arguments, resolved.
    pub command: Vec<String>,
    pub isolation: Isolation,
    /// Every part, with whether it is offered on this system.
    pub parts: Vec<(Part, bool)>,
}

impl UiSpec {
    fn fingerprint(&self) -> String {
        format!(
            "{:?} {} {:?}",
            self.command,
            self.isolation.fingerprint(),
            self.parts
        )
    }
}

/// One running interface process.
pub(super) struct Running {
    pub(super) id: u64,
    fingerprint: String,
    input: mpsc::UnboundedSender<ToPlugin>,
    owners: Arc<Mutex<Owners>>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub struct UiHost {
    specs: RwLock<BTreeMap<String, Arc<UiSpec>>>,
    /// The offered parts, made once per change: the status is read every 100 ms, and a
    /// shared list compares by pointer instead of by every icon's text.
    offered: RwLock<Arc<Vec<Part>>>,
    running: Arc<Mutex<BTreeMap<String, Running>>>,
    next: AtomicU64,
}

impl UiHost {
    /// The parts of the plugins that are on, as offered on this system.
    pub fn parts(&self) -> Arc<Vec<Part>> {
        Arc::clone(&self.offered.read())
    }

    /// The interfaces of the plugins that are on; one that went or changed stops.
    pub fn set(&self, specs: Vec<UiSpec>) {
        let specs: BTreeMap<String, Arc<UiSpec>> = specs
            .into_iter()
            .map(|spec| (spec.plugin.clone(), Arc::new(spec)))
            .collect();
        let mut running = self.running.lock();
        running.retain(|plugin, run| {
            let keep = specs
                .get(plugin)
                .is_some_and(|spec| spec.fingerprint() == run.fingerprint);
            if !keep {
                info!("interface of plugin {plugin} stops");
                run.task.abort();
                process::tell_all(plugin, &run.owners.lock(), "its interface stopped");
            }
            keep
        });
        let offered: Vec<Part> = specs
            .values()
            .flat_map(|spec| spec.parts.iter())
            .filter(|(_, offered)| *offered)
            .map(|(part, _)| part.clone())
            .collect();
        if **self.offered.read() != offered {
            *self.offered.write() = Arc::new(offered);
        }
        *self.specs.write() = specs;
    }

    /// Opens `part` for `client`, whose events go to `queue`; the plugin's view follows.
    pub fn open(
        &self,
        client: ClientId,
        queue: mpsc::UnboundedSender<Event>,
        plugin: &str,
        part: &str,
    ) -> Result<()> {
        let spec = self.offered(plugin, part)?;
        let mut running = self.running.lock();
        if !running.contains_key(plugin) {
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let started = process::start(id, &spec, Arc::clone(&self.running))?;
            info!("interface of plugin {plugin} starts");
            running.insert(
                plugin.to_owned(),
                Running {
                    id,
                    fingerprint: spec.fingerprint(),
                    input: started.input,
                    owners: started.owners,
                    task: started.task,
                },
            );
        }
        let run = running.get(plugin).context("the interface is gone")?;
        {
            let mut owners = run.owners.lock();
            let clients = owners.entry(part.to_owned()).or_default();
            if !clients.iter().any(|(id, _)| *id == client) {
                clients.push((client, queue));
            }
        }
        run.input
            .send(ToPlugin::Open {
                part: part.to_owned(),
            })
            .map_err(|_| anyhow::anyhow!("the interface of {plugin} stopped"))
    }

    /// Closes `part` for `client`; the plugin hears of it when nobody has it open.
    pub fn close(&self, client: ClientId, plugin: &str, part: &str) {
        let running = self.running.lock();
        if let Some(run) = running.get(plugin) {
            leave(run, client, Some(part));
        }
    }

    /// Passes on the use of a widget of a part `client` has open.
    pub fn event(
        &self,
        client: ClientId,
        plugin: &str,
        part: &str,
        widget: &str,
        event: UiEvent,
    ) -> Result<()> {
        ensure!(
            !widget.is_empty() && widget.len() <= MAX_WIDGET_ID,
            "invalid widget id"
        );
        if let UiEvent::Input { value } = &event {
            ensure!(value.chars().count() <= MAX_TEXT, "the input is too long");
        }
        let running = self.running.lock();
        let run = running
            .get(plugin)
            .filter(|run| {
                run.owners
                    .lock()
                    .get(part)
                    .is_some_and(|clients| clients.iter().any(|(id, _)| *id == client))
            })
            .with_context(|| format!("part {part} of {plugin} is not open"))?;
        run.input
            .send(ToPlugin::Event {
                part: part.to_owned(),
                widget: widget.to_owned(),
                event,
            })
            .map_err(|_| anyhow::anyhow!("the interface of {plugin} stopped"))
    }

    /// A client went away: the parts it had open close.
    pub fn close_client(&self, client: ClientId) {
        for run in self.running.lock().values() {
            leave(run, client, None);
        }
    }

    pub fn stop_all(&self) {
        for (_, run) in std::mem::take(&mut *self.running.lock()) {
            run.task.abort();
        }
    }

    fn offered(&self, plugin: &str, part: &str) -> Result<Arc<UiSpec>> {
        let spec = self
            .specs
            .read()
            .get(plugin)
            .cloned()
            .with_context(|| format!("no plugin {plugin} with an interface is on"))?;
        match spec.parts.iter().find(|(found, _)| found.id == part) {
            None => bail!("plugin {plugin} has no part {part}"),
            Some((_, false)) => bail!("part {part} of {plugin} is not offered on this system"),
            Some((_, true)) => Ok(spec),
        }
    }
}

/// `client` leaves `part`, or every part; a part nobody has open is closed.
fn leave(run: &Running, client: ClientId, part: Option<&str>) {
    let mut owners = run.owners.lock();
    let mut closed = Vec::new();
    for (name, clients) in owners.iter_mut() {
        if part.is_some_and(|part| part != name) {
            continue;
        }
        let before = clients.len();
        clients.retain(|(id, _)| *id != client);
        if before > 0 && clients.is_empty() {
            closed.push(name.clone());
        }
    }
    owners.retain(|_, clients| !clients.is_empty());
    drop(owners);
    for part in closed {
        let _ = run.input.send(ToPlugin::Close { part });
    }
}
