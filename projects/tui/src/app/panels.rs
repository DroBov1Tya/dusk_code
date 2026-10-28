//! Panels shown over the transcript: threads to open, switches for tools and skills, and
//! the keys.

use std::path::Path;

use dusk_core::{
    config::Config,
    graph::{Graph, SKILL_FILE},
    state::Thread,
};

pub enum Panel {
    Threads(ThreadList),
    Settings(SettingList),
    Help,
}

pub struct ThreadList {
    pub threads: Vec<Thread>,
    pub selected: usize,
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
