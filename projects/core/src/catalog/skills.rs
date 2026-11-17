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
    catalog::graph::{Graph, SKILL_FILE},
    config::Config,
};

pub use dusk_protocol::catalog::{SkillInfo, SkillKind, SkillSave, SkillSaveAction};

/// Every skill, the ones that cannot be read included, by name.
pub fn list(directory: &Path, config: &Config) -> Vec<SkillInfo> {
    let graph = Graph::load(directory, SKILL_FILE);
    let builtin = crate::release::resources::root()
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

/// The pack the built-in skills form; a store pack cannot take its name, since
/// `skills/duskcode` is a built-in skill's folder.
pub const BUILTIN_PACK: &str = "duskcode";

/// The pack a skill is in: the first folder of a nested one, `duskcode` for a built-in
/// one; none for a skill at the top that is not built in.
pub fn pack_of(name: &str) -> Option<String> {
    if let Some((pack, _)) = name.split_once('/') {
        return Some(pack.to_owned());
    }
    let builtin = crate::release::resources::root()
        .ok()
        .map(|root| root.join("skills"));
    is_builtin(name, builtin.as_deref()).then(|| BUILTIN_PACK.to_owned())
}

fn is_builtin(name: &str, builtin: Option<&Path>) -> bool {
    builtin.is_some_and(|root| root.join(name).join(SKILL_FILE).is_file())
}

fn kind(name: &str, has_source: bool, builtin: Option<&Path>) -> SkillKind {
    if is_builtin(name, builtin) {
        SkillKind::Builtin
    } else if has_source {
        SkillKind::Other
    } else {
        SkillKind::Own
    }
}
