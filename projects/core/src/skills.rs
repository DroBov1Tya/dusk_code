//! Skills as a client manages them: every folder under `skills/` with a `SKILL.md`, its
//! switch, and its text to read and edit. Skills are read from disk each time, so a
//! skill added or edited by hand applies at once and nothing is kept elsewhere.
//!
//! A skill is one of three kinds. Built-in skills ship beside dusk and are written to
//! `skills/` when missing; each credits its authors in its front matter and keeps their
//! license beside it. A skill whose front matter names a `source` came from elsewhere.
//! Every other skill is the user's own.

use std::path::Path;

use crate::{
    config::Config,
    graph::{Graph, SKILL_FILE},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillKind {
    Builtin,
    Own,
    Other,
}

impl SkillKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Builtin => "built-in",
            Self::Own => "own",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: SkillKind,
    /// The company or project it comes from, as its front matter says.
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    /// Why the skill cannot be used as it is; it can still be opened and fixed.
    pub problem: Option<String>,
}

/// Every skill, the ones that cannot be read included, by name.
pub fn list(directory: &Path, config: &Config) -> Vec<SkillInfo> {
    let graph = Graph::load(directory, SKILL_FILE);
    let builtin = crate::resources::root()
        .ok()
        .map(|root| root.join("skills"));
    let mut skills: Vec<SkillInfo> = graph
        .nodes()
        .map(|node| SkillInfo {
            name: node.name.clone(),
            description: node.description.clone(),
            kind: kind(&node.name, node.source.is_some(), builtin.as_deref()),
            source: node.source.clone(),
            tags: node.tags.clone(),
            enabled: config.skill_enabled(&node.name),
            problem: None,
        })
        .collect();
    for problem in graph.problems() {
        if graph.get(&problem.node).is_none() {
            skills.push(SkillInfo {
                name: problem.node.clone(),
                description: String::new(),
                kind: kind(&problem.node, false, builtin.as_deref()),
                source: None,
                tags: Vec::new(),
                enabled: config.skill_enabled(&problem.node),
                problem: Some(problem.reason.clone()),
            });
        }
    }
    skills.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    skills
}

fn kind(name: &str, has_source: bool, builtin: Option<&Path>) -> SkillKind {
    if builtin.is_some_and(|root| root.join(name).join("SKILL.md").is_file()) {
        SkillKind::Builtin
    } else if has_source {
        SkillKind::Other
    } else {
        SkillKind::Own
    }
}
