//! One interface process: lines to it, lines from it read with a hard cap, views checked,
//! frames decoded and paced, everything sent only to the clients that opened the part.

use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use dusk_protocol::{
    client::Event,
    plugin::{FromPlugin, MAX_FRAME, MAX_LINE, ToPlugin, check_view},
};
use parking_lot::Mutex;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};
use tracing::warn;

use super::host::{ClientId, Running, UiSpec};

/// The clients that opened each part, with their queues.
pub(super) type Owners = BTreeMap<String, Vec<(ClientId, mpsc::UnboundedSender<Event>)>>;

/// How long an interface nobody uses keeps running, for a part opened again soon.
const IDLE_GRACE: Duration = Duration::from_secs(10);
/// The most frames a screen sends a client a second; the newest wins between them.
const FRAME_PERIOD: Duration = Duration::from_millis(33);
const MAX_NOTICE: usize = 300;
const JPEG_START: [u8; 2] = [0xFF, 0xD8];

/// A process just started.
pub(super) struct Started {
    /// Where lines to it go.
    pub(super) input: mpsc::UnboundedSender<ToPlugin>,
    /// Who sees what it sends.
    pub(super) owners: Arc<Mutex<Owners>>,
    /// What runs it; it takes itself out of `running` when it ends.
    pub(super) task: JoinHandle<()>,
}

/// Starts the process of `spec`.
pub(super) fn start(
    id: u64,
    spec: &Arc<UiSpec>,
    running: Arc<Mutex<BTreeMap<String, Running>>>,
) -> Result<Started> {
    let mut child = spec
        .isolation
        .command(&spec.command)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("cannot start the interface of {}", spec.plugin))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let (input, mut lines) = mpsc::unbounded_channel::<ToPlugin>();
    let writer = tokio::spawn(async move {
        while let Some(message) = lines.recv().await {
            let Ok(mut line) = serde_json::to_string(&message) else {
                continue;
            };
            line.push('\n');
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let owners = Arc::new(Mutex::new(Owners::new()));
    let task = tokio::spawn({
        let owners = Arc::clone(&owners);
        let plugin = spec.plugin.clone();
        let parts: BTreeSet<String> = spec.parts.iter().map(|(part, _)| part.id.clone()).collect();
        async move {
            let mut reader = BufReader::new(stdout);
            let ended = serve(&plugin, &parts, &mut reader, &owners).await;
            writer.abort();
            drop(child);
            {
                let mut running = running.lock();
                if running.get(&plugin).is_some_and(|run| run.id == id) {
                    running.remove(&plugin);
                }
            }
            if let Some(why) = ended {
                warn!("interface of plugin {plugin}: {why}");
                tell_all(&plugin, &owners.lock(), &why);
            }
        }
    });
    Ok(Started {
        input,
        owners,
        task,
    })
}

/// Reads the process until it ends, sends a line too long or sits unused. Why it ended,
/// to tell the owners; none when it ended for want of use.
async fn serve(
    plugin: &str,
    parts: &BTreeSet<String>,
    reader: &mut (impl AsyncBufRead + Unpin),
    owners: &Mutex<Owners>,
) -> Option<String> {
    let mut line = Vec::new();
    let mut frames: BTreeMap<(String, String), Vec<u8>> = BTreeMap::new();
    let mut ticks = tokio::time::interval(FRAME_PERIOD);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut idle_since: Option<Instant> = None;
    loop {
        tokio::select! {
            read = read_line(reader, &mut line) => match read {
                Ok(true) => {
                    let taken = std::mem::take(&mut line);
                    take(plugin, parts, &taken, owners, &mut frames);
                }
                Ok(false) => return Some("its interface stopped".into()),
                Err(why) => return Some(why),
            },
            _ = ticks.tick() => {
                for ((part, widget), jpeg) in std::mem::take(&mut frames) {
                    send_to(owners, &part, |plugin_part| Event::PluginFrame {
                        plugin: plugin.to_owned(),
                        part: plugin_part,
                        widget: widget.clone(),
                        jpeg: jpeg.clone(),
                    });
                }
                if owners.lock().is_empty() {
                    let since = *idle_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= IDLE_GRACE {
                        return None;
                    }
                } else {
                    idle_since = None;
                }
            }
        }
    }
}

/// One line from the process, put to use; one that is not a message is dropped.
fn take(
    plugin: &str,
    parts: &BTreeSet<String>,
    line: &[u8],
    owners: &Mutex<Owners>,
    frames: &mut BTreeMap<(String, String), Vec<u8>>,
) {
    let message = match serde_json::from_slice::<FromPlugin>(line) {
        Ok(message) => message,
        Err(error) => {
            warn!("interface of plugin {plugin}: not a message: {error}");
            return;
        }
    };
    match message {
        FromPlugin::View { part, root } if parts.contains(&part) => match check_view(&root) {
            Ok(()) => send_to(owners, &part, |part| Event::PluginView {
                plugin: plugin.to_owned(),
                part,
                root: root.clone(),
            }),
            Err(why) => send_to(owners, &part, |_| Event::PluginNotice {
                plugin: plugin.to_owned(),
                text: format!("a view was refused: {why}"),
            }),
        },
        FromPlugin::Frame { part, widget, jpeg } if parts.contains(&part) => {
            match decode_frame(&jpeg) {
                Ok(jpeg) => {
                    frames.insert((part, widget), jpeg);
                }
                Err(why) => send_to(owners, &part, |_| Event::PluginNotice {
                    plugin: plugin.to_owned(),
                    text: format!("a frame was refused: {why}"),
                }),
            }
        }
        FromPlugin::Notice { text } => {
            let text: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_NOTICE)
                .collect();
            tell_all(plugin, &owners.lock(), &text);
        }
        FromPlugin::View { part, .. } | FromPlugin::Frame { part, .. } => {
            warn!("interface of plugin {plugin}: no part {part}");
        }
    }
}

fn decode_frame(base64: &str) -> Result<Vec<u8>, String> {
    // Four characters carry three bytes; anything longer cannot fit.
    if base64.len() > MAX_FRAME.div_ceil(3) * 4 {
        return Err("it is too large".into());
    }
    let jpeg = STANDARD
        .decode(base64)
        .map_err(|_| "it is not base64".to_owned())?;
    if jpeg.len() > MAX_FRAME {
        return Err("it is too large".into());
    }
    if !jpeg.starts_with(&JPEG_START) {
        return Err("it is not a JPEG".into());
    }
    Ok(jpeg)
}

/// Reads up to the end of a line into `line`, which keeps what came across a cancel.
/// False at the end of the stream; the reason when a line grows past `MAX_LINE`.
async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    line: &mut Vec<u8>,
) -> Result<bool, String> {
    loop {
        let buffer = reader
            .fill_buf()
            .await
            .map_err(|error| format!("cannot read its interface: {error}"))?;
        if buffer.is_empty() {
            return Ok(false);
        }
        let (used, done) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (buffer.len(), false),
        };
        let content = if done { used - 1 } else { used };
        if line.len() + content > MAX_LINE {
            return Err(format!(
                "its interface sent a line over {} MiB and was stopped",
                MAX_LINE / (1024 * 1024)
            ));
        }
        line.extend_from_slice(&buffer[..content]);
        reader.consume(used);
        if done {
            return Ok(true);
        }
    }
}

/// Sends what `event` makes of the part's name to each client that opened it.
fn send_to(owners: &Mutex<Owners>, part: &str, event: impl Fn(String) -> Event) {
    let owners = owners.lock();
    for (_, queue) in owners.get(part).into_iter().flatten() {
        let _ = queue.send(event(part.to_owned()));
    }
}

/// A note to every client that has a part of the plugin open, once each.
pub(super) fn tell_all(plugin: &str, owners: &Owners, text: &str) {
    let mut told = BTreeSet::new();
    for (client, queue) in owners.values().flatten() {
        if told.insert(*client) {
            let _ = queue.send(Event::PluginNotice {
                plugin: plugin.to_owned(),
                text: text.to_owned(),
            });
        }
    }
}
