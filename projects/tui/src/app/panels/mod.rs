//! Panels shown over the transcript: threads to open with a preview of each, switches
//! for tools and skills, and the keys.

use std::path::Path;

use ratatui::layout::Rect;

use dusk_core::{
    config::Config,
    graph::{Graph, SKILL_FILE},
    runtime::model_choice::ProviderStatus,
    state::{Message, MessageRole, Thread},
};

pub mod agents;

pub use agents::{AgentWindow, SubagentView};

use super::{Input, PasteEditor};

pub enum Panel {
    Threads(ThreadList),
    Settings(SettingList),
    Help,
    Paste(PasteEditor),
    Models(ModelList),
    Agent(AgentWindow),
    Connect(Connection),
    Unsent { selected: usize },
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

pub struct SettingList {
    pub max_agents_at_work: usize,
    pub items: Vec<Setting>,
    /// `None` is the category menu, where values cannot be changed.
    pub category: Option<SettingCategory>,
    pub selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingCategory {
    Agents,
    Tools,
    Skills,
}

impl SettingCategory {
    pub const ALL: [Self; 3] = [Self::Agents, Self::Tools, Self::Skills];

    pub fn name(self) -> &'static str {
        match self {
            Self::Agents => "agents",
            Self::Tools => "tools",
            Self::Skills => "skills",
        }
    }

    pub fn kind(self) -> Option<SettingKind> {
        match self {
            Self::Agents => None,
            Self::Tools => Some(SettingKind::Tool),
            Self::Skills => Some(SettingKind::Skill),
        }
    }
}

pub struct Setting {
    pub kind: SettingKind,
    pub name: String,
    pub about: String,
    pub on: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    Tool,
    Skill,
}

impl SettingList {
    pub fn load(config: &Config, tools: &[String], skills_dir: &Path) -> Self {
        let tools = tools.iter().map(|name| Setting {
            kind: SettingKind::Tool,
            name: name.clone(),
            about: String::new(),
            on: config.tool_enabled(name),
        });
        let graph = Graph::load(skills_dir, SKILL_FILE);
        let skills = graph.nodes().map(|skill| Setting {
            kind: SettingKind::Skill,
            name: skill.name.clone(),
            about: skill.description.clone(),
            on: config.skill_enabled(&skill.name),
        });
        Self {
            max_agents_at_work: config.limits().max_agents_at_work,
            items: tools.chain(skills).collect(),
            category: None,
            selected: 0,
        }
    }

    pub fn len(&self) -> usize {
        match self.category {
            None => SettingCategory::ALL.len(),
            Some(SettingCategory::Agents) => 1,
            Some(category) => self
                .items
                .iter()
                .filter(|item| Some(item.kind) == category.kind())
                .count(),
        }
    }

    pub fn open(&mut self) {
        if self.category.is_none() {
            self.category = SettingCategory::ALL.get(self.selected).copied();
            self.selected = 0;
        }
    }

    pub fn back(&mut self) {
        if let Some(category) = self.category.take() {
            self.selected = SettingCategory::ALL
                .iter()
                .position(|item| *item == category)
                .unwrap_or(0);
        }
    }
}

/// Moves a selection within `len` items, stopping at the ends.
pub fn step(selected: &mut usize, len: usize, down: bool) {
    if down {
        *selected = (*selected + 1).min(len.saturating_sub(1));
    } else {
        *selected = selected.saturating_sub(1);
    }
}
