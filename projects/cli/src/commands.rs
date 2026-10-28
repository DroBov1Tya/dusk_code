//! Client commands that start with `:`. They change settings through the core and never
//! reach the model: `:tools`, `:skills`, `:on <name>`, `:off <name>`, `:reload`.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, bail};
use dusk_core::{
    graph::{Graph, SKILL_FILE},
    live_config::LiveConfig,
};

pub const PREFIX: char = ':';

pub struct Settings {
    pub config: Arc<LiveConfig>,
    pub tools: Vec<&'static str>,
    pub skills_dir: PathBuf,
}

/// What `:on` and `:off` switch.
enum Target {
    Tool,
    Skill,
}

/// Runs a command line without the prefix and returns what to show the user.
pub fn run(line: &str, settings: &Settings) -> Result<String> {
    let mut words = line.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("tools"), None, _) => Ok(tool_list(settings)),
        (Some("skills"), None, _) => Ok(skill_list(settings)),
        (Some("on"), Some(name), None) => switch(settings, name, true),
        (Some("off"), Some(name), None) => switch(settings, name, false),
        (Some("reload"), None, _) => {
            settings.config.reload()?;
            Ok("config reloaded".into())
        }
        _ => bail!("unknown command; use :tools, :skills, :on <name>, :off <name> or :reload"),
    }
}

fn tool_list(settings: &Settings) -> String {
    let config = settings.config.current();
    settings
        .tools
        .iter()
        .map(|&tool| format!("{} {tool}", state(config.tool_enabled(tool))))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every skill with its state, then what is wrong with the graph.
fn skill_list(settings: &Settings) -> String {
    let config = settings.config.current();
    let graph = Graph::load(&settings.skills_dir, SKILL_FILE);
    let mut lines: Vec<String> = graph
        .nodes()
        .map(|skill| {
            let entry = if skill.entry { " (entry)" } else { "" };
            let mut line = format!(
                "{} {}{entry}: {}",
                state(config.skill_enabled(&skill.name)),
                skill.name,
                skill.description
            );
            if !skill.links.is_empty() {
                line.push_str(&format!("\n      -> {}", skill.links.join(", ")));
            }
            line
        })
        .collect();
    lines.extend(
        graph
            .problems()
            .iter()
            .map(|problem| format!("!   {}: {}", problem.node, problem.reason)),
    );
    if lines.is_empty() {
        return format!("no skills in {}", settings.skills_dir.display());
    }
    lines.join("\n")
}

fn switch(settings: &Settings, name: &str, enabled: bool) -> Result<String> {
    let is_tool = settings.tools.contains(&name);
    let is_skill = Graph::load(&settings.skills_dir, SKILL_FILE)
        .get(name)
        .is_some();
    let target = match (is_tool, is_skill) {
        (true, false) => Target::Tool,
        (false, true) => Target::Skill,
        (true, true) => bail!("{name} is both a tool and a skill; rename the skill folder"),
        (false, false) => bail!("unknown tool or skill {name}; :tools and :skills list them"),
    };
    settings.config.update(|config| match target {
        Target::Tool => config.set_tool_enabled(name, enabled),
        Target::Skill => config.set_skill_enabled(name, enabled),
    })?;
    Ok(format!("{name} is {}", state(enabled).trim_end()))
}

fn state(enabled: bool) -> &'static str {
    if enabled { "on " } else { "off" }
}
