//! `load_skill`: how the model walks the skill graph. Its description lists the entry
//! skills; loading a skill returns its instructions and the skills it leads to.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{
    schema,
    schema::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{
    BoxFuture,
    catalog::{
        graph::{Graph, Node, SKILL_FILE},
        skills::{BUILTIN_PACK, builtin_dir, is_builtin},
    },
    config::Config,
};

/// The graph is described to the model several times a step; reading it again this
/// soon would find the same files.
const GRAPH_TTL: Duration = Duration::from_secs(5);
/// Each skill listed costs tokens in every request.
const MAX_LISTED: usize = 60;
const MAX_LISTED_CHARS: usize = 400;

/// The skill that edits skills; its name is all the reminder needs.
const EVOLVE: &str = "skill-evolve";
const REMINDER: &str = "If what worked in this task differs from these steps or beats them, \
                        update this skill with skill-evolve before you answer.";

pub struct LoadSkill {
    directory: PathBuf,
    graph: Mutex<Option<Cached>>,
}

/// The graph read at that moment, with where the store's skills came from.
type Cached = (Instant, Arc<Graph>, Arc<Origins>);

use crate::plugins::store::origin::{self, Origins};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LoadSkillArguments {
    /// Node name, as listed.
    name: String,
}

impl LoadSkill {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            graph: Mutex::default(),
        }
    }

    fn graph(&self) -> (Arc<Graph>, Arc<Origins>) {
        let mut cached = self.graph.lock();
        match &*cached {
            Some((read, graph, unverified)) if read.elapsed() < GRAPH_TTL => {
                (Arc::clone(graph), Arc::clone(unverified))
            }
            _ => {
                let graph = Arc::new(Graph::load(&self.directory, SKILL_FILE));
                let marks = Arc::new(Origins::beside(&self.directory));
                *cached = Some((Instant::now(), Arc::clone(&graph), Arc::clone(&marks)));
                (graph, marks)
            }
        }
    }
}

impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self, context: &ToolContext) -> String {
        let (graph, marks) = self.graph();
        let builtin = builtin_dir();
        let entries = enabled(graph.entries(), &context.config, builtin.as_deref(), &marks);
        format!(
            "Load the instructions of a skill when the task matches its description, and \
             follow them. To use a skill, load it with this tool by name rather than reading \
             its file. A \
             loaded skill may name further skills; load those with this tool as well.\n\
             Skills:\n{entries}"
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<LoadSkillArguments>()
    }

    fn available(&self, context: &ToolContext) -> bool {
        let builtin = builtin_dir();
        self.graph().0.entries().any(|skill| {
            context
                .config
                .skill_enabled_in(&skill.name, builtin.as_deref())
        })
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
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
    // Read once: every check below needs it, and resolving it validates the resources.
    let builtin = builtin_dir();
    let builtin = builtin.as_deref();
    let skill = graph
        .get(name)
        .filter(|skill| config.skill_enabled_in(&skill.name, builtin))
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
    let marks = Origins::beside(directory);
    let from = marks.unverified("skills", name);
    let instructions = match from {
        Some((_, source)) => origin::wrap(
            "skill",
            source,
            &format!("The files of this skill are in {folder}.\n\n{instructions}"),
        ),
        None => format!("The files of this skill are in {folder}.\n\n{instructions}"),
    };
    // An Unverified skill leads only to skills of its own folder: never to a built-in
    // one, such as skill-evolve, which edits skills, nor to the user's own.
    let links = graph.links(skill).filter(|link| match from {
        Some((root, _)) => marks
            .unverified("skills", &link.name)
            .is_some_and(|(other, _)| other == root),
        None => true,
    });
    let next = enabled(links, config, builtin, &marks);
    let mut text = if next.is_empty() {
        instructions
    } else {
        format!("{instructions}\n\nNext skills; load any of them with load_skill:\n{next}")
    };
    if from.is_none() && wants_reminder(&graph, name, config, builtin) {
        text.push_str("\n\n");
        text.push_str(REMINDER);
    }
    Ok(text)
}

/// The reminder goes on a skill the user can edit: not the editing skills themselves,
/// not a built-in one (a release replaces it), and only while skill-evolve is usable.
fn wants_reminder(graph: &Graph, name: &str, config: &Config, builtin: Option<&Path>) -> bool {
    // skill-evolve ships under `resources/skills/`, so it is in the built-in pack: its
    // own switch and the pack's decide.
    name != EVOLVE
        && name != "skill-creator"
        && config.skill_switch(EVOLVE)
        && config.pack_enabled(BUILTIN_PACK)
        && graph.get(EVOLVE).is_some()
        && is_own(name, builtin)
}

/// A name with a `/` is a user's own sub-folder: built-in skills are top-level only.
/// For a top-level one the built-in folder must be known to tell; when it is not, no
/// reminder is better than one on a built-in skill.
fn is_own(name: &str, builtin: Option<&Path>) -> bool {
    name.contains('/') || builtin.is_some_and(|folder| !is_builtin(name, Some(folder)))
}

/// Switched-off skills are left out, wherever they would be shown.
fn enabled<'a>(
    skills: impl Iterator<Item = &'a Node>,
    config: &Config,
    builtin: Option<&Path>,
    origins: &Origins,
) -> String {
    let skills: Vec<&Node> = skills
        .filter(|skill| config.skill_enabled_in(&skill.name, builtin))
        .collect();
    let mut listed: Vec<String> = skills
        .iter()
        .take(MAX_LISTED)
        .map(|skill| {
            let description: String = skill.description.chars().take(MAX_LISTED_CHARS).collect();
            let source = origins
                .unverified("skills", &skill.name)
                .map(|(_, source)| source);
            origin::listed(&skill.name, source, &description)
        })
        .collect();
    if skills.len() > MAX_LISTED {
        listed.push(format!("({} more not listed)", skills.len() - MAX_LISTED));
    }
    listed.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_dir, tool_context};
    use crate::tools::ToolScope;

    fn skills_dir() -> PathBuf {
        let dir = temp_dir("load_skill_reminder");
        for name in [EVOLVE, "mine"] {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(SKILL_FILE),
                format!("---\nname: {name}\ndescription: d\n---\nSteps.\n"),
            )
            .unwrap();
        }
        dir
    }

    fn config(dir: &Path) -> Config {
        let scope = ToolScope::new(dir, dir).unwrap();
        (*tool_context(&scope).config).clone()
    }

    #[test]
    fn reminder_ends_an_own_skill_only() {
        let dir = skills_dir();
        let config = config(&dir);
        assert!(load(&dir, "mine", &config).unwrap().ends_with(REMINDER));
        assert!(!load(&dir, EVOLVE, &config).unwrap().contains(REMINDER));
    }

    #[test]
    fn no_reminder_when_skill_evolve_is_off() {
        let dir = skills_dir();
        let mut config = config(&dir);
        config.set_skill_enabled(EVOLVE, false);
        assert!(!load(&dir, "mine", &config).unwrap().contains(REMINDER));
    }

    #[test]
    fn no_reminder_when_the_builtin_folder_is_unknown() {
        let dir = skills_dir();
        // Unreadable resources: a top-level skill might be built in, so none is
        // reminded; a sub-folder skill never is.
        assert!(!is_own("mine", None));
        assert!(is_own("pack/mine", None));
        // A readable folder tells built-in from own.
        assert!(!is_own(EVOLVE, Some(&dir)));
        assert!(is_own("other", Some(&dir)));
    }

    /// A skill from an Unverified source reaches the model as a third party's text, gets
    /// no reminder to edit it, and leads only to skills of its own folder.
    #[test]
    fn an_unverified_skill_is_marked_and_kept_to_its_folder() {
        let paths = crate::testing::temp_paths("load_skill_unverified");
        let dir = paths.skills_dir();
        let skill = |name: &str, front: &str| {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(SKILL_FILE),
                format!("---\ndescription: d\n{front}---\nSteps.\n"),
            )
            .unwrap();
        };
        skill(EVOLVE, "");
        skill("mine", "");
        skill("loose", "links: [mine, skill-evolve, loose/inner]\n");
        skill("loose/inner", "entry: false\n");
        std::fs::write(
            paths.config_root().join("store.json"),
            r#"{"serial": 0, "installed": {"loose": {"version": "1.0.0", "sha256": "ab",
                "source": "far", "url": "https://far.example.com/index.json", "key": "",
                "kind": "skill", "folder": "skills/loose"}}}"#,
        )
        .unwrap();
        let config = config(&dir);
        let text = load(&dir, "loose", &config).unwrap();
        assert!(
            text.starts_with("[third-party text] This skill comes from far, an unverified"),
            "{text}"
        );
        assert!(text.contains("- loose/inner"), "{text}");
        assert!(
            !text.contains("- mine") && !text.contains("- skill-evolve"),
            "{text}"
        );
        assert!(!text.contains(REMINDER));
        // The user's own skill reads as before.
        let mine = load(&dir, "mine", &config).unwrap();
        assert!(mine.starts_with("The files of this skill"));
        let listed = enabled(
            Graph::load(&dir, SKILL_FILE).entries(),
            &config,
            None,
            &Origins::beside(&dir),
        );
        assert!(
            listed.contains("- loose (unverified, from far): d"),
            "{listed}"
        );
    }
}
