//! A device's screen as it changes, for the panel: AXe streams JPEG frames, each after
//! its length as four bytes, and the newest one is kept for whoever asks next. A stream
//! starts on the first ask and stops when nobody has asked for a few seconds. Without
//! AXe, every ask takes one picture through simctl instead.

use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::system::SimScreen;
use parking_lot::Mutex;
use tokio::{io::AsyncReadExt, sync::watch};

use super::{run::command, screen};

const FPS: &str = "30";
/// Half the device's pixels is still sharper than the panel draws it.
const SCALE: &str = "0.5";
const QUALITY: &str = "70";
/// A frame larger than this is not a JPEG of a phone's screen.
const MAX_FRAME: usize = 8 * 1024 * 1024;
/// How long an ask waits for a newer frame; a still screen sends none.
const WAIT: Duration = Duration::from_secs(2);
/// A stream nobody asked for this long is stopped.
const IDLE: Duration = Duration::from_secs(5);

#[derive(Clone, Default)]
struct Frame {
    seq: u64,
    jpeg: Arc<Vec<u8>>,
}

struct Stream {
    id: u64,
    frames: watch::Receiver<Frame>,
    asked: Arc<Mutex<Instant>>,
}

static STREAMS: LazyLock<Mutex<HashMap<String, Stream>>> = LazyLock::new(Mutex::default);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The first frame newer than `after`, or the newest one when the screen holds still.
/// `udid` has been checked by `run::device`.
pub async fn next(udid: &str, after: u64, axe: bool) -> Result<SimScreen> {
    let (width, height) = screen::points(udid).await?;
    if !axe {
        let mut shot = screen::take(udid).await?;
        shot.seq = after + 1;
        return Ok(shot);
    }
    let mut frames = watch(udid);
    let fresh = tokio::time::timeout(WAIT, async {
        frames
            .wait_for(|frame| frame.seq > after)
            .await
            .map(|frame| frame.clone())
    })
    .await;
    let frame = match fresh {
        Ok(Ok(frame)) => frame,
        Ok(Err(_)) => anyhow::bail!("the screen stream stopped"),
        Err(_) => frames.borrow().clone(),
    };
    ensure!(frame.seq > 0, "the device sent no picture yet");
    Ok(SimScreen {
        jpeg: frame.jpeg.as_ref().clone(),
        width,
        height,
        seq: frame.seq,
    })
}

/// The device's frames, starting its stream when none runs.
fn watch(udid: &str) -> watch::Receiver<Frame> {
    let mut streams = STREAMS.lock();
    if let Some(stream) = streams.get(udid) {
        *stream.asked.lock() = Instant::now();
        return stream.frames.clone();
    }
    let (sender, frames) = watch::channel(Frame::default());
    let asked = Arc::new(Mutex::new(Instant::now()));
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let stream = Stream {
        id,
        frames: frames.clone(),
        asked: Arc::clone(&asked),
    };
    streams.insert(udid.to_owned(), stream);
    let udid = udid.to_owned();
    tokio::spawn(async move {
        if let Err(error) = run(&udid, sender, asked).await {
            tracing::debug!("the screen stream of {udid} ended: {error:#}");
        }
        let mut streams = STREAMS.lock();
        if streams.get(&udid).is_some_and(|stream| stream.id == id) {
            streams.remove(&udid);
        }
    });
    frames
}

async fn run(udid: &str, frames: watch::Sender<Frame>, asked: Arc<Mutex<Instant>>) -> Result<()> {
    let mut axe = command("axe");
    axe.args([
        "stream-video",
        "--udid",
        udid,
        "--format",
        "raw",
        "--fps",
        FPS,
        "--scale",
        SCALE,
        "--quality",
        QUALITY,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = axe.spawn().context("AXe did not start")?;
    let mut out = child.stdout.take().context("AXe gave no output")?;
    let mut idle = tokio::time::interval(IDLE / 2);
    let mut seq = 0;
    loop {
        tokio::select! {
            frame = read_frame(&mut out) => {
                seq += 1;
                frames.send_replace(Frame { seq, jpeg: Arc::new(frame?) });
            }
            _ = idle.tick() => {
                if asked.lock().elapsed() > IDLE {
                    return Ok(());
                }
            }
        }
    }
}

async fn read_frame(out: &mut tokio::process::ChildStdout) -> Result<Vec<u8>> {
    let mut length = [0u8; 4];
    out.read_exact(&mut length)
        .await
        .context("the stream ended")?;
    let length = u32::from_be_bytes(length) as usize;
    ensure!(
        (1..=MAX_FRAME).contains(&length),
        "a frame of {length} bytes"
    );
    let mut frame = vec![0; length];
    out.read_exact(&mut frame)
        .await
        .context("the stream ended")?;
    Ok(frame)
}
