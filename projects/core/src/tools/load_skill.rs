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
    skills::{Skill, SkillGraph},
};

pub(super) struct LoadSkill {
    directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSkillArguments {
    name: String,
}

impl LoadSkill {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let graph = SkillGraph::load(&self.directory);
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
                "name": { "type": "string", "description": "Skill name, as listed." }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn available(&self, context: &ToolContext<'_>) -> bool {
        SkillGraph::load(&self.directory)
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
    let graph = SkillGraph::load(directory);
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
fn enabled<'a>(skills: impl Iterator<Item = &'a Skill>, config: &Config) -> String {
    skills
        .filter(|skill| config.skill_enabled(&skill.name))
        .map(|skill| format!("- {}: {}", skill.name, skill.description))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{
        testing::{temp_dir, tool_context},
        tools::ToolScope,
    };

    fn skills(name: &str) -> (LoadSkill, ToolScope) {
        let root = temp_dir(name);
        let directory = root.join("skills");
        for (path, text) in [
            (
                "start",
                "---\ndescription: Where to begin\nentry: true\nlinks: [deep/next]\n---\nBegin.",
            ),
            (
                "deep/next",
                "---\ndescription: The next step\n---\nContinue.",
            ),
        ] {
            fs::create_dir_all(directory.join(path)).unwrap();
            fs::write(directory.join(path).join("SKILL.md"), text).unwrap();
        }
        let scope = ToolScope::new(&root, &root).unwrap();
        (LoadSkill::new(directory), scope)
    }

    #[test]
    fn lists_only_entry_skills() {
        let (tool, scope) = skills("load_skill_list");
        let description = tool.description(&tool_context(&scope));
        assert!(description.contains("- start: Where to begin"));
        assert!(!description.contains("deep/next"));
        assert!(tool.available(&tool_context(&scope)));
    }

    #[tokio::test]
    async fn loading_a_skill_shows_where_it_leads() {
        let (tool, scope) = skills("load_skill_walk");
        let start = tool
            .call(json!({ "name": "start" }), tool_context(&scope))
            .await
            .unwrap();
        assert_eq!(
            start,
            "Begin.\n\nNext skills; load any of them with load_skill:\n- deep/next: The next step"
        );
        let next = tool
            .call(json!({ "name": "deep/next" }), tool_context(&scope))
            .await
            .unwrap();
        assert_eq!(next, "Continue.");
        assert!(
            tool.call(json!({ "name": "nope" }), tool_context(&scope))
                .await
                .is_err()
        );
    }

    #[test]
    fn switched_off_skills_are_hidden() {
        let (tool, scope) = skills("load_skill_off");
        let mut config = tool_context(&scope).config.clone();
        config.set_skill_enabled("start", false);
        let context = ToolContext {
            scope: &scope,
            config: &config,
        };
        assert!(!tool.available(&context));
        let mut config = tool_context(&scope).config.clone();
        config.set_skill_enabled("deep/next", false);
        assert!(load(&tool.directory, "deep/next", &config).is_err());
        assert_eq!(load(&tool.directory, "start", &config).unwrap(), "Begin.");
    }
}
