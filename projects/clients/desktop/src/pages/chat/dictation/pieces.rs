//! Cutting what was heard into pieces a server takes: at a pause, mono, 16 kHz, as WAV.

use super::microphone::Heard;
use std::time::Duration;

/// A piece is cut at a pause once it is this long, and cut anyway at the longer one.
pub(super) const CUT_AFTER: Duration = Duration::from_secs(4);

pub(super) const CUT_ANYWAY: Duration = Duration::from_secs(12);

/// How much of the end must be quiet to count as a pause.
pub(super) const PAUSE: Duration = Duration::from_millis(450);

/// Under this level (RMS of samples from -1 to 1) a stretch is silence.
pub(super) const QUIET: f32 = 0.01;

/// What is sent to the server.
pub(super) const SERVED_RATE: u32 = 16_000;

/// Under this peak a whole piece is silence.
pub(super) const SILENT_PEAK: f32 = 0.02;

/// Cuts what was heard into a WAV when it is time: at a pause after `CUT_AFTER`, after
/// `CUT_ANYWAY`, or at the end. Stretches of silence are dropped, as servers invent words
/// for them.
pub(super) fn take_piece(heard: &mut Heard, ending: bool) -> Option<Vec<u8>> {
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
