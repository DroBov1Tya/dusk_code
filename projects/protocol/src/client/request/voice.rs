//! Recordings in, speech out.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceRequest {
    /// Turns a WAV recording into text.
    Transcribe { audio: Vec<u8> },
    /// Reads text aloud; answers with the sound as a card of the artifact store.
    Speak { text: String },
}
