//! Recordings in, speech out.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceRequest {
    /// Turns a WAV recording into text.
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
