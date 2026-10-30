//! Panels shown over the transcript: threads to open with a preview of each, the
//! settings, and the keys.

use ratatui::layout::Rect;

use dusk_core::{
    chat::commands::ContextReport,
    release::update::UpdateInfo,
    runtime::model_choice::ProviderStatus,
    state::{Message, MessageRole, Thread},
};

pub mod agents;
pub mod settings;

pub use agents::{AgentWindow, SubagentView};
pub use settings::{
    AgentRow, FileButton, FileEditor, HitTarget, SettingCategory, SettingsHit, SettingsView,
    SwitchKind,
};

use super::{Input, PasteEditor};

pub enum Panel {
    Threads(ThreadList),
    Settings(Box<SettingsView>),
    Help,
    Paste(PasteEditor),
    Models(ModelList),
    Agent(AgentWindow),
    Connect(Connection),
    Unsent {
        selected: usize,
    },
    /// The newest version and the button that installs it; `info` is `None` while the
    /// core asks GitHub, and `note` says what the last step did.
    Update {
        info: Option<UpdateInfo>,
        note: Option<String>,
        /// The update button on the last drawn frame, as (row, left, right).
        button: Option<(u16, u16, u16)>,
    },
    /// The reasoning efforts to choose from: default first.
    Effort {
        selected: usize,
    },
    /// What the context holds; `row` is the line kept in view while scrolling.
    Context {
        report: ContextReport,
        row: usize,
    },
}

/// The configured providers as the core found them, and their models to choose from.
pub struct ModelList {
    /// `provider/model` in use.
    pub current: Option<String>,
    /// `None` while the core checks the providers.
    pub providers: Option<Vec<ProviderStatus>>,
    /// Among `choices`.
    pub selected: usize,
}

impl ModelList {
    pub fn checking(current: Option<String>) -> Self {
        Self {
            current,
            providers: None,
            selected: 0,
        }
    }

    pub fn checked(current: Option<String>, providers: Vec<ProviderStatus>) -> Self {
        let mut list = Self {
            current,
            providers: Some(providers),
            selected: 0,
        };
        list.selected = list
            .choices()
            .iter()
            .position(|name| Some(name) == list.current.as_ref())
            .unwrap_or(0);
        list
    }

    /// Every model of every provider that answered, as `provider/model`, in order.
    pub fn choices(&self) -> Vec<String> {
        let providers = self.providers.iter().flatten();
        providers
            .flat_map(|provider| {
                let models = provider.models.as_deref().unwrap_or_default();
                models
                    .iter()
                    .map(|model| format!("{}/{}", provider.name, model.id))
            })
            .collect()
    }
}

pub struct ThreadList {
    pub threads: Vec<Thread>,
    pub selected: usize,
    /// The selected thread at a glance, once loaded.
    pub preview: Option<Preview>,
}

/// What a thread is about without its whole history: how it began and how it stands.
pub struct Preview {
    pub thread_id: String,
    /// User messages and answers with text, oldest first.
    pub messages: Vec<(MessageRole, String)>,
    pub tool_calls: usize,
}

impl Preview {
    pub fn new(thread_id: String, history: &[Message]) -> Self {
        let messages = history
            .iter()
            .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
            .filter(|message| !message.content.trim().is_empty())
            .map(|message| (message.role, message.content.clone()))
            .collect();
        let tool_calls = history.iter().map(|message| message.tool_calls.len()).sum();
        Self {
            thread_id,
            messages,
            tool_calls,
        }
    }
}

#[derive(Default)]
pub struct Connection {
    pub fields: [Input; 2],
    pub selected: usize,
    pub error: Option<String>,
    /// Visible label and value rows on the last drawn frame.
    pub field_areas: [Option<Rect>; 2],
}

/// Moves a selection within `len` items, stopping at the ends.
pub fn step(selected: &mut usize, len: usize, down: bool) {
    if down {
        *selected = (*selected + 1).min(len.saturating_sub(1));
    } else {
        *selected = selected.saturating_sub(1);
    }
}
