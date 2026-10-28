//! `load_skill`: how the model walks the skill graph. Its description lists the entry
//! skills; loading a skill returns its instructions and the skills it leads to.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    builtin::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{
    BoxFuture,
    config::Config,
    graph::{Graph, Node, SKILL_FILE},
};

pub struct LoadSkill {
    directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSkillArguments {
    name: String,
}

impl LoadSkill {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let graph = Graph::load(&self.directory, SKILL_FILE);
        let entries = enabled(graph.entries(), context.config);
        format!(
            "Load the instructions of a skill when the task matches its description, and \
             follow them. Skills are not files: load them only with this tool, by name. A \
             loaded skill may name further skills; load those with this tool as well.\n\
             Skills:\n{entries}"
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Node name, as listed." }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn available(&self, context: &ToolContext<'_>) -> bool {
        Graph::load(&self.directory, SKILL_FILE)
            .entries()
            .any(|skill| context.config.skill_enabled(&skill.name))
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: LoadSkillArguments = parse_arguments(arguments)?;
            let directory = self.directory.clone();
            let config = context.config.clone();
            tokio::task::spawn_blocking(move || load(&directory, &arguments.name, &config))
                .await
                .context("cannot load the skill")?
        })
    }
}

fn load(directory: &Path, name: &str, config: &Config) -> Result<String> {
    let graph = Graph::load(directory, SKILL_FILE);
    let skill = graph
        .get(name)
        .filter(|skill| config.skill_enabled(&skill.name))
        .with_context(|| format!("no skill named {name}"))?;
    let instructions = skill.instructions()?;
    ensure!(!instructions.is_empty(), "skill {name} has no instructions");
    let next = enabled(graph.links(skill), config);
    if next.is_empty() {
        return Ok(instructions);
    }
    Ok(format!(
        "{instructions}\n\nNext skills; load any of them with load_skill:\n{next}"
    ))
}

/// Switched-off skills are left out, wherever they would be shown.
fn enabled<'a>(skills: impl Iterator<Item = &'a Node>, config: &Config) -> String {
    skills
        .filter(|skill| config.skill_enabled(&skill.name))
        .map(|skill| format!("- {}: {}", skill.name, skill.description))
        .collect::<Vec<_>>()
        .join("\n")
}
