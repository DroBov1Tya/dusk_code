//! The client state: the open thread as entries, what is being typed, what the agent is
//! doing and which panel is open. Events from the core and keys from the user change it;
//! anything that needs the core goes back to the session as an `Action`.

mod action;
mod composer;
mod events;
mod hits;
mod keys;
mod mouse;
mod panels;
mod state;
mod submit;
mod thread;

pub use action::{Action, commands};
pub use composer::PathPopup;
pub use composer::{EditorButton, EditorHit, EditorTarget, Input, PasteEditor, paste};
pub use hits::{
    Activity, Button, ButtonHit, ChipHit, Focus, InputRow, PRESS, Pressed, Selection, TEXT_COLUMN,
    TranscriptHit,
};
pub use keys::EFFORTS;
pub use panels::{
    AgentRow, AgentWindow, Connection, DiffView, FileButton, FileEditor, HitTarget, JobsView,
    McpRow, ModelList, Panel, PluginRow, Preset, Preview, SecretEntry, SettingCategory,
    SettingsHit, SettingsView, SubagentView, SwitchKind, ThreadList,
};
pub use state::{App, compact_number, now_ms};
pub use thread::approval::{ApprovalHit, Asked};
pub use thread::goal::{GoalButton, GoalHit, buttons as goal_buttons};
pub use thread::poll::{Poll, PollHit, PollTarget};
pub use thread::queue::{QueueButton, QueueHit};
pub use thread::transcript::{Entry, ImageSpot, Layout, ShownPreview, Tone, Transcript};
