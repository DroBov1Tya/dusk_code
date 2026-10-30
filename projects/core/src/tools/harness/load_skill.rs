//! `load_skill`: how the model walks the skill graph. Its description lists the entry
//! skills; loading a skill returns its instructions and the skills it leads to.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{
    builtin::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{
    BoxFuture,
    catalog::graph::{Graph, Node, SKILL_FILE},
    config::Config,
    util::sync::lock,
};

/// The graph is described to the model several times a step; reading it again this
/// soon would find the same files.
const GRAPH_TTL: Duration = Duration::from_secs(5);
/// Each skill listed costs tokens in every request.
const MAX_LISTED: usize = 60;
const MAX_LISTED_CHARS: usize = 400;

pub struct LoadSkill {
    directory: PathBuf,
    graph: Mutex<Option<(Instant, Arc<Graph>)>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSkillArguments {
    name: String,
}

impl LoadSkill {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            graph: Mutex::default(),
        }
    }

    fn graph(&self) -> Arc<Graph> {
        let mut cached = lock(&self.graph);
        match &*cached {
            Some((read, graph)) if read.elapsed() < GRAPH_TTL => Arc::clone(graph),
            _ => {
                let graph = Arc::new(Graph::load(&self.directory, SKILL_FILE));
                *cached = Some((Instant::now(), Arc::clone(&graph)));
                graph
            }
        }
    }
}

impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let graph = self.graph();
        let entries = enabled(graph.entries(), context.config);
        format!(
            "Load the instructions of a skill when the task matches its description, and \
             follow them. To use a skill, load it with this tool by name rather than reading \
             its file. A \
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
        self.graph()
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
        .with_context(|| {
            format!(
                "no skill named {name} in {}; only the skills listed in this tool's \
                 description can be loaded",
                directory.display()
            )
        })?;
    let instructions = skill.instructions()?;
    ensure!(!instructions.is_empty(), "skill {name} has no instructions");
    // Skills name their own scripts and references by paths inside their folder.
    let folder = skill.file().parent().unwrap_or(directory).display();
    let instructions = format!("The files of this skill are in {folder}.\n\n{instructions}");
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
    let skills: Vec<&Node> = skills
        .filter(|skill| config.skill_enabled(&skill.name))
        .collect();
    let mut listed: Vec<String> = skills
        .iter()
        .take(MAX_LISTED)
        .map(|skill| {
            let description: String = skill.description.chars().take(MAX_LISTED_CHARS).collect();
            format!("- {}: {description}", skill.name)
        })
        .collect();
    if skills.len() > MAX_LISTED {
        listed.push(format!("({} more not listed)", skills.len() - MAX_LISTED));
    }
    listed.join("\n")
}
