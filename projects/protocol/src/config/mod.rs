//! What the user configures: policies, memory, providers, secrets, speech, MCP servers, plugins
//! and the profile.

mod mcp;
mod memory;
mod mention;
mod panel;
mod plugin;
mod policy;
mod provider;
mod secrets;
mod version_control;
mod voice;
mod voice_models;

pub use mcp::{McpInfo, McpState, McpStatus, McpToolInfo};
pub use memory::{Memory, WorkHistory, default_memory_chars, yes};
pub use mention::{MAX_NAME_CHARS, ME, Profile, ProfileFile, Snippet, find, is_name, is_name_char};
pub use panel::{ModeInfo, Settings, ToolInfo, ToolKind};
pub use plugin::PluginInfo;
pub use policy::{ApprovalPolicy, ContextPolicy, ContextPreset, UNKNOWN_WINDOW};
pub use provider::ProviderKind;
pub use secrets::{HUGGINGFACE, SecretInfo, SecretsInfo};
pub use version_control::VersionControl;
pub use voice::{
    MAX_RECORDING_BYTES, MAX_SPEECH_CHARS, STT_KEY, TTS_KEY, VoiceEndpoint, VoiceSettings,
};
pub use voice_models::{LocalState, SpeechKind, VoiceModel, VoiceModels};
