//! Speech: where a client's recordings are turned into text and its text into sound. Both
//! ends speak the OpenAI audio API (`/audio/transcriptions`, `/audio/speech`), which the
//! hosted services and the local servers share, so one pair of settings covers a cloud
//! account and a model that runs on this machine.

use serde::{Deserialize, Serialize};

/// One server and the model to ask of it. The key is kept apart, in the keychain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceEndpoint {
    pub base_url: String,
    pub model: String,
    /// The voice of a synthesis server; unused for recognition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

/// What is set up: recognition and synthesis are independent, and either may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stt: Option<VoiceEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts: Option<VoiceEndpoint>,
}

impl VoiceSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The secrets of the two servers' keys.
pub const STT_KEY: &str = "voice/stt";

pub const TTS_KEY: &str = "voice/tts";

/// The largest recording a client may send, in bytes.
pub const MAX_RECORDING_BYTES: usize = 25 * 1024 * 1024;

/// The longest text that is read aloud at once, in characters.
pub const MAX_SPEECH_CHARS: usize = 4096;
