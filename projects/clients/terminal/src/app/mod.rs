//! The client state: the open thread as entries, what is being typed, what the agent is
//! doing and which panel is open. Events from the core and keys from the user change it;
//! anything that needs the core goes back to the session as an `Action`. Other open
//! threads wait in tabs (`tabs`).

mod action;
mod composer;
mod events;
mod hits;
mod keys;
mod mouse;
mod panels;
pub mod schedule;
mod state;
mod submit;
mod tabs;
mod thread;

pub use action::{Action, PinAsk, commands};
pub use composer::PathPopup;
pub use composer::{EditorButton, EditorHit, EditorTarget, Input, PasteEditor, paste};
pub use hits::{
    Activity, Button, ButtonHit, ChipHit, Focus, InputRow, PRESS, Pressed, Selection, TEXT_COLUMN,
    TranscriptHit,
};
pub use keys::EFFORTS;
pub use panels::{
    AgentRow, AgentWindow, AtOnceEdit, Connection, DiffView, FileButton, FileEditor, HitTarget,
    JobsView, McpRow, ModelList, OverviewView, Panel, PluginRow, Preset, Preview, SecretEntry,
    SettingCategory, SettingsHit, SettingsView, SourceReader, SubagentView, SwitchKind, ThreadList,
};
pub use panels::{
    Choice, Confirm, PackPage, Reviewed, Row, Step, Workshop, choices, order_word, reviewed_first,
};
pub use state::{App, compact_number};
#[cfg(test)]
pub(crate) use tabs::tests::app as test_app;
pub use tabs::{Tab, TabHit, TabMove};
#[cfg(test)]
pub(crate) use thread::approval::tests::batch as approval_batch;
pub use thread::approval::{ApprovalHit, Asked};
pub use thread::goal::{GoalButton, GoalHit, buttons as goal_buttons};
pub use thread::poll::{Poll, PollHit, PollTarget};
pub use thread::queue::{QueueButton, QueueHit};
pub use thread::transcript::{Entry, ImageSpot, Layout, ShownPreview, Tone, Transcript};
