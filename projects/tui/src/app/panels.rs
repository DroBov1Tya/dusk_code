//! Panels shown over the transcript: threads to open with a preview of each, switches
//! for tools and skills, and the keys.

use std::path::Path;

use dusk_core::{
    config::Config,
    graph::{Graph, SKILL_FILE},
    state::{Message, MessageRole, Thread},
};

pub enum Panel {
    Threads(ThreadList),
    Settings(SettingList),
    Help,
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

pub struct SettingList {
    pub items: Vec<Setting>,
    pub selected: usize,
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
    pub fn load(config: &Config, tools: &[&str], skills_dir: &Path) -> Self {
        let tools = tools.iter().map(|&name| Setting {
            kind: SettingKind::Tool,
            name: name.to_owned(),
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
            items: tools.chain(skills).collect(),
            selected: 0,
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
