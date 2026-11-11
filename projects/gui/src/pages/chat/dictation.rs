//! Dictation: the microphone is read while the button is on, cut into pieces at a pause, and
//! each piece goes to the speech recognition server; its words are added to the message box
//! as they come back. A piece is a WAV of 16 kHz mono, the least every server takes.
//! Pieces are sent one at a time, so the words keep their order.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use rodio::{Source, microphone::MicrophoneBuilder};

use gpui_kit::*;

use crate::workspace::Workspace;

/// A piece is cut at a pause once it is this long, and cut anyway at the longer one.
const CUT_AFTER: Duration = Duration::from_secs(4);
const CUT_ANYWAY: Duration = Duration::from_secs(12);
/// How much of the end must be quiet to count as a pause.
const PAUSE: Duration = Duration::from_millis(450);
/// Under this level (RMS of samples from -1 to 1) a stretch is silence.
const QUIET: f32 = 0.01;
/// What is sent to the server.
const SERVED_RATE: u32 = 16_000;
/// Pieces waiting for a server that is slower than the speech: past this, the dictation ends.
const MAX_QUEUE: usize = 8;
/// After the stop, a microphone that sends nothing for this long is taken as ended.
const STALLED: Duration = Duration::from_secs(2);
/// Under this peak a whole piece is silence.
const SILENT_PEAK: f32 = 0.02;

/// What the window knows of a dictation in progress.
pub struct Dictation {
    stop: Arc<AtomicBool>,
    heard: Arc<Mutex<Heard>>,
    /// Pieces cut and not yet sent.
    queue: VecDeque<Vec<u8>>,
    /// A piece is with the server.
    busy: bool,
    /// The microphone gave nothing yet; its failure to open is waited for.
    started: std::time::Instant,
}

#[derive(Default)]
struct Heard {
    rate: u32,
    channels: u16,
    /// Interleaved samples not yet cut into a piece.
    samples: Vec<f32>,
    /// Why the microphone could not be opened or stopped.
    error: Option<String>,
    /// The microphone thread has ended.
    ended: bool,
    /// When samples last arrived.
    last: Option<std::time::Instant>,
}

/// The microphone's samples go into `heard` until `stop` is set. Runs on a thread of its own,
/// since the stream is read by blocking.
fn listen(stop: Arc<AtomicBool>, heard: Arc<Mutex<Heard>>) {
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

impl Drop for Dictation {
    /// A dictation that goes out of the window's hands, as with the window itself, must not
    /// leave the microphone on.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Workspace {
    pub fn dictating(&self) -> bool {
        self.dictation.is_some()
    }

    /// Starts or ends the dictation.
    pub fn toggle_dictation(&mut self, cx: &mut Context<Self>) {
        if let Some(dictation) = &self.dictation {
            // The thread ends on its next sample; what it heard is cut and sent first.
            dictation.stop.store(true, Ordering::Relaxed);
            return;
        }
        // The microphone is opened only when a server is there to take what it hears.
        self.when_voice_ready(
            crate::pages::connectors::voice::Speech::Recognition,
            cx,
            Box::new(|ws, cx| ws.start_dictation(cx)),
        );
    }

    fn start_dictation(&mut self, cx: &mut Context<Self>) {
        if self.dictation.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let heard = Arc::new(Mutex::new(Heard::default()));
        let (thread_stop, thread_heard) = (Arc::clone(&stop), Arc::clone(&heard));
        if let Err(error) = std::thread::Builder::new()
            .name("dictation".into())
            .spawn(move || listen(thread_stop, thread_heard))
        {
            self.fail(anyhow::anyhow!("cannot start the microphone: {error}"), cx);
            return;
        }
        self.dictation = Some(Dictation {
            stop,
            heard,
            queue: VecDeque::new(),
            busy: false,
            started: std::time::Instant::now(),
        });
        self.watch_dictation(cx);
        cx.notify();
    }

    /// Looks at the microphone four times a second: cuts pieces, sends them, notices the end.
    fn watch_dictation(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let go = ws.update(cx, |ws, cx| ws.dictation_step(cx));
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look; false when the dictation is over.
    fn dictation_step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(dictation) = self.dictation.as_mut() else {
            return false;
        };
        let stopping = dictation.stop.load(Ordering::Relaxed);
        let (piece, ended, mut error) = {
            let mut heard = dictation.heard.lock();
            // A microphone that stalls after the stop never sends the sample that ends its
            // loop; it is taken as ended.
            let quiet_for = heard
                .last
                .map_or(dictation.started.elapsed(), |at| at.elapsed());
            if stopping && !heard.ended && quiet_for > STALLED {
                heard.ended = true;
            }
            let ending = stopping || heard.ended;
            let piece = take_piece(&mut heard, ending);
            (piece, heard.ended, heard.error.take())
        };
        if let Some(piece) = piece {
            dictation.queue.push_back(piece);
        }
        if dictation.queue.len() > MAX_QUEUE {
            dictation.queue.clear();
            dictation.stop.store(true, Ordering::Relaxed);
            error.get_or_insert_with(|| "the speech server is too slow for dictation".into());
        }
        let finished = ended && dictation.queue.is_empty() && !dictation.busy;
        if let Some(error) = error {
            self.dictation = None;
            self.fail(anyhow::anyhow!(error), cx);
            cx.notify();
            return false;
        }
        if finished {
            self.dictation = None;
            cx.notify();
            return false;
        }
        self.send_next_piece(cx);
        cx.notify();
        true
    }

    fn send_next_piece(&mut self, cx: &mut Context<Self>) {
        let Some(dictation) = self.dictation.as_mut() else {
            return;
        };
        if dictation.busy {
            return;
        }
        let Some(piece) = dictation.queue.pop_front() else {
            return;
        };
        dictation.busy = true;
        self.core.call(
            cx,
            move |client| async move { client.transcribe(piece).await },
            |ws, result, cx| {
                if let Some(dictation) = ws.dictation.as_mut() {
                    dictation.busy = false;
                }
                match result {
                    Ok(text) if !text.is_empty() => {
                        let said = ws.composer.read(cx).value().to_string();
                        let joined = match said.trim_end() {
                            "" => text,
                            before => format!("{before} {text}"),
                        };
                        ws.fill_composer = Some(joined);
                    }
                    Ok(_) => {}
                    // A server that fails once is likely to fail again: the dictation ends.
                    Err(error) => {
                        if let Some(dictation) = ws.dictation.as_mut() {
                            dictation.stop.store(true, Ordering::Relaxed);
                            dictation.queue.clear();
                        }
                        ws.fail(error, cx);
                    }
                }
                cx.notify();
            },
        );
    }
}

/// Cuts what was heard into a WAV when it is time: at a pause after `CUT_AFTER`, after
/// `CUT_ANYWAY`, or at the end. Stretches of silence are dropped, as servers invent words
/// for them.
fn take_piece(heard: &mut Heard, ending: bool) -> Option<Vec<u8>> {
    if heard.rate == 0 || heard.channels == 0 {
        return None;
    }
    let per_second = heard.rate as usize * heard.channels as usize;
    let length = Duration::from_secs_f32(heard.samples.len() as f32 / per_second as f32);
    let tail = (PAUSE.as_secs_f32() * per_second as f32) as usize;
    let at_pause = length >= CUT_AFTER
        && heard.samples.len() >= tail
        && rms(&heard.samples[heard.samples.len() - tail..]) < QUIET;
    if !(ending || at_pause || length >= CUT_ANYWAY) {
        return None;
    }
    let samples = std::mem::take(&mut heard.samples);
    // Silence is told by the loudest sample, so quiet speech in a long piece is kept.
    let peak = samples
        .iter()
        .fold(0f32, |peak, sample| peak.max(sample.abs()));
    if samples.is_empty() || peak < SILENT_PEAK {
        return None;
    }
    let mono = to_mono(&samples, heard.channels);
    Some(wav(&resample(&mono, heard.rate, SERVED_RATE), SERVED_RATE))
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Linear interpolation, enough for speech.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let step = from as f64 / to as f64;
    let count = (samples.len() as f64 / step) as usize;
    (0..count)
        .map(|n| {
            let at = n as f64 * step;
            let i = at as usize;
            let fraction = (at - i as f64) as f32;
            let next = samples.get(i + 1).copied().unwrap_or(samples[i]);
            samples[i] * (1. - fraction) + next * fraction
        })
        .collect()
}

/// A mono 16-bit PCM WAV.
fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1., 1.) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring gpui's own `test` macro over the standard one.
    use super::{Heard, QUIET, resample, rms, take_piece, to_mono, wav};

    fn heard(seconds: f32, level: f32) -> Heard {
        let rate = 16_000;
        Heard {
            rate,
            channels: 1,
            samples: vec![level; (rate as f32 * seconds) as usize],
            ..Heard::default()
        }
    }

    #[test]
    fn a_wav_has_its_header_and_one_two_byte_sample_each() {
        let bytes = wav(&[0., 1., -1.], 16_000);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            16_000
        );
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(
            i16::from_le_bytes(bytes[46..48].try_into().unwrap()),
            i16::MAX
        );
    }

    #[test]
    fn stereo_is_averaged_and_rates_are_scaled() {
        assert_eq!(to_mono(&[1., 0., 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(resample(&vec![0.; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
    }

    #[test]
    fn a_piece_is_cut_at_a_pause_not_in_speech() {
        // Five seconds of speech: no pause yet, nothing is cut.
        let mut speaking = heard(5., 0.2);
        assert!(take_piece(&mut speaking, false).is_none());
        // The same, with a quiet end: cut.
        let mut paused = heard(5., 0.2);
        let quiet = paused.samples.len() - 8_000;
        paused.samples[quiet..].fill(0.);
        assert!(take_piece(&mut paused, false).is_some());
        assert!(paused.samples.is_empty());
        // Twelve seconds without a pause is cut anyway.
        assert!(take_piece(&mut heard(12.5, 0.2), false).is_some());
    }

    #[test]
    fn silence_is_never_sent_and_the_end_flushes_the_rest() {
        assert!(take_piece(&mut heard(6., 0.), true).is_none());
        assert!(rms(&vec![QUIET / 2.; 100]) < QUIET);
        assert!(take_piece(&mut heard(1., 0.2), true).is_some());
        assert!(take_piece(&mut Heard::default(), true).is_none());
    }
}
