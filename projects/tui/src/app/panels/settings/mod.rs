mod editor;
mod input;
mod types;
mod view;

pub use editor::FileEditor;
pub use types::{
    AgentRow, FileButton, HitTarget, McpRow, SettingCategory, SettingsHit, SwitchKind,
};
pub use view::SettingsView;
