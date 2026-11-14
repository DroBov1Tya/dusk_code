//! Recordings in, speech out.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceRequest {
    /// Turns a WAV recording into text; `wav` makes one from samples.
    Transcribe { audio: Vec<u8> },
    /// Reads text aloud; answers with the sound as a card of the artifact store.
    Speak { text: String },
    /// The speech models of this machine and where each one is.
    Models,
    /// Installs a model with `uv`, starts it and sets the chat's speech to it. Answers at
    /// once; `Models` shows the progress.
    Install { id: String },
    /// Stops an install that is under way and deletes what it had made.
    Cancel { id: String },
    /// Stops a model, deletes it and what it downloaded.
    Remove { id: String },
}

/// A mono 16-bit PCM WAV of samples from -1 to 1 at `rate` Hz, as `Transcribe` takes it.
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
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
    use super::wav;

    #[test]
    fn a_wav_has_its_header_and_one_two_byte_sample_each() {
        let bytes = wav(&[0., 1., -2.], 16_000);
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
        assert_eq!(
            i16::from_le_bytes(bytes[48..50].try_into().unwrap()),
            -i16::MAX
        );
    }
}
