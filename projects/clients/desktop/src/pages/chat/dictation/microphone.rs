//! Reading the microphone on a thread of its own.

use gpui_kit::*;
use parking_lot::Mutex;
use rodio::{Source, microphone::MicrophoneBuilder};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(super) struct Heard {
    pub(super) rate: u32,
    pub(super) channels: u16,
    /// Interleaved samples not yet cut into a piece.
    pub(super) samples: Vec<f32>,
    /// Why the microphone could not be opened or stopped.
    pub(super) error: Option<String>,
    /// The microphone thread has ended.
    pub(super) ended: bool,
    /// When samples last arrived.
    pub(super) last: Option<std::time::Instant>,
}

/// The microphone's samples go into `heard` until `stop` is set. Runs on a thread of its own,
/// since the stream is read by blocking.
pub(super) fn listen(stop: Arc<AtomicBool>, heard: Arc<Mutex<Heard>>) {
    let opened = || -> Result<_, String> {
        let builder = MicrophoneBuilder::new()
            .default_device()
            .map_err(|error| error.to_string())?;
        let builder = builder
            .default_config()
            .map_err(|error| error.to_string())?;
        builder.open_stream().map_err(|error| error.to_string())
    };
    let microphone = match opened() {
        Ok(microphone) => microphone,
        Err(error) => {
            let mut heard = heard.lock();
            heard.error = Some(format!("cannot open the microphone: {error}"));
            heard.ended = true;
            return;
        }
    };
    {
        let mut heard = heard.lock();
        heard.rate = microphone.sample_rate().get();
        heard.channels = microphone.channels().get();
    }
    let mut batch = Vec::with_capacity(4096);
    for sample in microphone {
        batch.push(sample);
        if batch.len() >= 2048 {
            let mut shared = heard.lock();
            shared.samples.append(&mut batch);
            shared.last = Some(std::time::Instant::now());
        }
        // Nobody listens any more: the window dropped the dictation.
        if stop.load(Ordering::Relaxed) || Arc::strong_count(&heard) == 1 {
            break;
        }
    }
    let mut heard = heard.lock();
    heard.samples.append(&mut batch);
    heard.ended = true;
}
