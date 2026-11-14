//! The settings panel: the state of its rows and selection (`view`), what keys, paste and
//! the mouse do in it (`input`), the editor of a skill, agent or MCP server file
//! (`editor`), and the plain types they share (`types`). Saving goes back to the session
//! as an `Action`.

mod editor;
mod input;
mod types;
mod view;

pub use editor::FileEditor;
pub use types::{
    AgentRow, FileButton, HitTarget, McpRow, SettingCategory, SettingsHit, SwitchKind,
};
pub use view::SettingsView;
