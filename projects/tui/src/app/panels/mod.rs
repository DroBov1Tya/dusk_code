//! Panels shown over the transcript: threads to open with a preview of each, the
//! settings, and the keys.

pub mod agents;
mod panel;
pub mod settings;

pub use agents::{AgentWindow, SubagentView};
pub use panel::{
    BotRow, Connection, JobsView, ModelList, Panel, Preset, Preview, ThreadList, step,
};
pub use settings::{
    AgentRow, FileButton, FileEditor, HitTarget, McpRow, SettingCategory, SettingsHit,
    SettingsView, SwitchKind,
};
