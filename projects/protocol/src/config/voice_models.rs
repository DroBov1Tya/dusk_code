//! The speech models the app can install and run on this machine, as a client lists them.

use serde::{Deserialize, Serialize};

/// Which way a model works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechKind {
    /// Speech into text: the microphone.
    Recognition,
    /// Text into speech: reading aloud.
    Synthesis,
}

/// Where a model is on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalState {
    NotInstalled,
    /// What it does now, in a few words.
    Installing(String),
    /// Installed, and not running.
    Installed,
    Running,
    /// Why the last install or start did not work.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceModel {
    pub id: String,
    pub title: String,
    pub about: String,
    pub kind: SpeechKind,
    /// What it takes on disk, such as "1.6 GB".
    pub size: String,
    pub languages: String,
    pub state: LocalState,
    /// The chat's speech settings point at it.
    pub in_use: bool,
}

/// The models, and whether `uv`, which installs them, is on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceModels {
    pub uv_found: bool,
    pub models: Vec<VoiceModel>,
}
