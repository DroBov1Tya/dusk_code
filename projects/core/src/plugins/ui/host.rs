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
/// The longest name of a key, such as `forward_delete`.
const MAX_KEY: usize = 32;

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
        let mut restart = Vec::new();
        running.retain(|plugin, run| {
            let spec = specs.get(plugin);
            if spec.is_some_and(|spec| spec.fingerprint() == run.fingerprint) {
                return true;
            }
            info!("interface of plugin {plugin} stops");
            run.task.abort();
            let mut owners = run.owners.lock();
            if let Some(spec) = spec
                && let Some((part, _)) = spec.parts.iter().find(|(part, offered)| {
                    *offered && super::PhoneDependency::for_panel(plugin, &part.id).is_some()
                })
                && let Some(clients) = owners.remove(&part.id)
            {
                restart.push((spec, part.id.clone(), clients));
            }
            process::tell_all(plugin, &owners, "its interface stopped");
            false
        });
        for (spec, part, clients) in restart {
            if clients.is_empty() {
                continue;
            }
            match self.start(spec) {
                Ok(run) => {
                    run.owners.lock().insert(part.clone(), clients);
                    if run.input.send(ToPlugin::Open { part }).is_err() {
                        process::tell_all(
                            &spec.plugin,
                            &run.owners.lock(),
                            "its interface stopped",
                        );
                    }
                    running.insert(spec.plugin.clone(), run);
                }
                Err(error) => {
                    let owners = Owners::from([(part, clients)]);
                    process::tell_all(&spec.plugin, &owners, &format!("{error:#}"));
                }
            }
        }
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
            running.insert(plugin.to_owned(), self.start(&spec)?);
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

    fn start(&self, spec: &Arc<UiSpec>) -> Result<Running> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let started = process::start(id, spec, Arc::clone(&self.running))?;
        info!("interface of plugin {} starts", spec.plugin);
        Ok(Running {
            id,
            fingerprint: spec.fingerprint(),
            input: started.input,
            owners: started.owners,
            task: started.task,
        })
    }

    pub fn generation(&self, plugin: &str) -> Option<u64> {
        self.running.lock().get(plugin).map(|run| run.id)
    }

    /// Refreshes an open part, or opens it once if a rescan stopped its process.
    pub fn redetect(
        &self,
        client: ClientId,
        queue: mpsc::UnboundedSender<Event>,
        plugin: &str,
        part: &str,
        previous: Option<u64>,
    ) -> Result<()> {
        if self.resumed(client, plugin, part, previous)
            || self
                .event(client, plugin, part, "refresh", UiEvent::Click)
                .is_ok()
        {
            return Ok(());
        }
        self.open(client, queue, plugin, part)
    }

    fn resumed(&self, client: ClientId, plugin: &str, part: &str, previous: Option<u64>) -> bool {
        self.running.lock().get(plugin).is_some_and(|run| {
            Some(run.id) != previous
                && run
                    .owners
                    .lock()
                    .get(part)
                    .is_some_and(|clients| clients.iter().any(|(owner, _)| *owner == client))
        })
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
        match &event {
            UiEvent::Input { value: text } | UiEvent::Text { text } => {
                ensure!(text.chars().count() <= MAX_TEXT, "the input is too long");
            }
            UiEvent::Key { key } => ensure!(
                !key.is_empty()
                    && key.len() <= MAX_KEY
                    && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "invalid key"
            ),
            _ => {}
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
