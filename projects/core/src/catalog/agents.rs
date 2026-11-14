//! The agent graph. Every folder under `agents/` with an `AGENT.md` is an expert agent:
//! its text is the system prompt of the threads it works in. The folder is read at the
//! start and then every minute, and what is found is recorded as identities, with a
//! version that grows when a file changes. A chat asks the entry agents; an agent asks
//! the agents it links to. The built-in general agent may be asked from anywhere.
//! Switching an agent off in the config hides it from every thread.

use std::{path::PathBuf, sync::Arc, time::Duration};

use parking_lot::RwLock;

use anyhow::{Context, Result, ensure};
use tracing::warn;

use crate::{
    catalog::{
        graph::{AGENT_FILE, Graph, Problem, front_skills, read_text, split_front_matter},
        skills::SkillInfo,
    },
    config::Config,
    release::resources,
    runtime::thread::start::fnv1a,
    state::{Identity, State},
};

pub use dusk_protocol::catalog::{AgentNode, AgentTree};

/// The built-in agent, asked when none is named. An `agents/general` folder replaces it.
pub const GENERAL_AGENT: &str = "general";
pub const GENERAL_DESCRIPTION: &str = "works like the chat in MAIN mode, with every tool; \
for research, reading code and self-contained parts of a task";
/// A chat is at depth 0; an agent three levels down can no longer ask another.
pub const MAX_AGENT_DEPTH: usize = 3;

pub struct Agents {
    directory: PathBuf,
    state: Arc<State>,
    identities: RwLock<Vec<Identity>>,
    problems: RwLock<Vec<Problem>>,
    skipped: RwLock<Vec<Problem>>,
}

impl Agents {
    pub fn new(directory: PathBuf, state: Arc<State>) -> Arc<Self> {
        Arc::new(Self {
            directory,
            state,
            identities: RwLock::default(),
            problems: RwLock::default(),
            skipped: RwLock::default(),
        })
    }

    /// Reads `agents/` and records what is there.
    pub async fn scan(&self) -> Result<()> {
        let directory = self.directory.clone();
        let (found, problems, skipped) =
            tokio::task::spawn_blocking(move || read_agents(&directory))
                .await
                .context("cannot read the agents")?;
        let identities = self.state.sync_identities(&found).await?;
        *self.identities.write() = identities;
        *self.problems.write() = problems;
        *self.skipped.write() = skipped;
        Ok(())
    }

    /// Reads the folder again every `period`, so new and edited agents apply.
    pub async fn watch(self: Arc<Self>, period: Duration) {
        let mut ticks = tokio::time::interval(period);
        ticks.tick().await;
        loop {
            ticks.tick().await;
            if let Err(error) = self.scan().await {
                warn!("cannot read the agents: {error:#}");
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<Identity> {
        self.identities
            .read()
            .iter()
            .find(|identity| identity.name == name)
            .cloned()
    }

    /// Whom a thread may ask: the entries for a chat (`asker` is `None`), the links for an
    /// agent. Switched-off agents are left out.
    pub fn reachable(&self, asker: Option<&str>, config: &Config) -> Vec<Identity> {
        let identities = self.identities.read();
        let names: Vec<String> = match asker {
            None => identities
                .iter()
                .filter(|identity| identity.entry)
                .map(|identity| identity.name.clone())
                .collect(),
            Some(asker) => identities
                .iter()
                .find(|identity| identity.name == asker)
                .map(|identity| identity.links.clone())
                .unwrap_or_default(),
        };
        identities
            .iter()
            .filter(|identity| names.contains(&identity.name))
            .filter(|identity| config.agent_enabled(&identity.name))
            .cloned()
            .collect()
    }

    /// Whether the built-in general agent may be asked: it is on and no AGENT.md
    /// replaces it.
    pub fn builtin_general(&self, config: &Config) -> bool {
        config.agent_enabled(GENERAL_AGENT) && self.get(GENERAL_AGENT).is_none()
    }

    /// The agent's prompt: its file, which its identity names, and what every agent is
    /// told. `skills` are those it can load now. A file edited since the last read is
    /// recorded first, so the identity returned is the version the prompt is.
    pub async fn prompt(&self, name: &str, skills: &[SkillInfo]) -> Result<(Identity, String)> {
        let (identity, file) = self.read_current(name).await?;
        // An agent whose links are all missing works alone, and is told so.
        let mut present = identity.clone();
        present.links.retain(|link| self.get(link).is_some());
        let prompt = compose(&present, &file, skills)?;
        Ok((identity, prompt))
    }

    async fn read_current(&self, name: &str) -> Result<(Identity, AgentFile)> {
        let identity = self.get(name).with_context(|| format!("no agent {name}"))?;
        let (text, hash) = read_prompt(&identity)?;
        if hash == identity.content_hash {
            return Ok((identity, text));
        }
        self.scan().await?;
        let identity = self
            .get(name)
            .with_context(|| format!("agent {name} is gone"))?;
        let (text, hash) = read_prompt(&identity)?;
        ensure!(
            hash == identity.content_hash,
            "agent {name} changes while it is read"
        );
        Ok((identity, text))
    }

    /// The chat and every agent it can reach, each drawn once: under the chat when the
    /// chat may ask it, else under the nearest agent that links to it. `links` names
    /// whom each may ask, wherever those are drawn.
    pub fn tree(&self, config: &Config) -> AgentTree {
        let identities = self.identities.read().clone();
        let mut placed: Vec<(&str, Option<&str>)> = Vec::new();
        let mut level: Vec<&str> = identities
            .iter()
            .filter(|identity| identity.entry)
            .map(|identity| identity.name.as_str())
            .collect();
        placed.extend(level.iter().map(|name| (*name, None)));
        for _ in 1..MAX_AGENT_DEPTH {
            let mut next = Vec::new();
            for asker in level {
                for link in links(asker, &identities) {
                    if placed.iter().all(|(name, _)| *name != link) {
                        placed.push((link, Some(asker)));
                        next.push(link);
                    }
                }
            }
            level = next;
        }
        let mut children = Vec::new();
        if self.get(GENERAL_AGENT).is_none() {
            children.push(AgentNode {
                name: GENERAL_AGENT.into(),
                description: GENERAL_DESCRIPTION.into(),
                version: None,
                enabled: config.agent_enabled(GENERAL_AGENT),
                links: Vec::new(),
                children: Vec::new(),
            });
        }
        children.extend(branches(None, &placed, &identities, config));
        AgentTree {
            root: AgentNode {
                name: "chat".into(),
                description: "the chat, in its mode".into(),
                version: None,
                enabled: true,
                links: Vec::new(),
                children,
            },
            problems: self.problems.read().clone(),
            skipped: self.skipped.read().clone(),
        }
    }
}

/// The agents `asker` links to that exist.
fn links<'a>(asker: &str, all: &'a [Identity]) -> Vec<&'a str> {
    let Some(identity) = all.iter().find(|identity| identity.name == asker) else {
        return Vec::new();
    };
    identity
        .links
        .iter()
        .filter_map(|link| all.iter().find(|other| other.name == *link))
        .map(|other| other.name.as_str())
        .collect()
}

/// The nodes drawn under `parent`, with theirs below them.
fn branches(
    parent: Option<&str>,
    placed: &[(&str, Option<&str>)],
    all: &[Identity],
    config: &Config,
) -> Vec<AgentNode> {
    placed
        .iter()
        .filter(|(_, above)| *above == parent)
        .filter_map(|(name, _)| all.iter().find(|identity| identity.name == *name))
        .map(|identity| AgentNode {
            name: identity.name.clone(),
            description: identity.description.clone(),
            version: Some(identity.version),
            enabled: config.agent_enabled(&identity.name),
            links: links(&identity.name, all)
                .into_iter()
                .map(Into::into)
                .collect(),
            children: branches(Some(&identity.name), placed, all, config),
        })
        .collect()
}

fn read_agents(directory: &std::path::Path) -> (Vec<Identity>, Vec<Problem>, Vec<Problem>) {
    let graph = Graph::load(directory, AGENT_FILE);
    let skipped = graph.skipped().to_vec();
    let mut problems = graph.problems().to_vec();
    let mut found = Vec::new();
    for node in graph.nodes() {
        match read_text(node.file()) {
            Ok(text) => found.push(Identity {
                name: node.name.clone(),
                path: node.file().display().to_string(),
                version: 0,
                content_hash: fingerprint(text.as_bytes()),
                description: node.description.clone(),
                entry: node.entry,
                links: node.links.clone(),
                tools: node.tools.clone(),
                updated_at: 0,
            }),
            Err(error) => problems.push(Problem {
                node: node.name.clone(),
                reason: format!("{error:#}"),
            }),
        }
    }
    (found, problems, skipped)
}

/// What an agent's file says for its prompt.
struct AgentFile {
    body: String,
    skills: Vec<String>,
}

/// The file's prompt, and the fingerprint of the whole file.
fn read_prompt(identity: &Identity) -> Result<(AgentFile, String)> {
    let text = read_text(std::path::Path::new(&identity.path))
        .with_context(|| format!("cannot read agent {}", identity.name))?;
    let (_, body) = split_front_matter(&text)?;
    let file = AgentFile {
        body: body.trim().to_owned(),
        skills: front_skills(&text),
    };
    Ok((file, fingerprint(text.as_bytes())))
}

fn may_use(identity: &Identity, tool: &str) -> bool {
    identity
        .tools
        .as_ref()
        .is_none_or(|tools| tools.iter().any(|name| name == tool))
}

/// The agent's own text, then what all agents share: the rules, how far it delegates, and
/// the skills it can load, so a skill switched off or gone is never pointed to.
fn compose(identity: &Identity, file: &AgentFile, skills: &[SkillInfo]) -> Result<String> {
    let rules = resources::text("internal/AGENT.md")?;
    let delegates = !identity.links.is_empty() && may_use(identity, "ask_agent");
    let reach = resources::text(match delegates {
        true => "internal/AGENT_LINKS.md",
        false => "internal/AGENT_LEAF.md",
    })?;
    let mut prompt = format!("{}\n\n{}\n\n{}", file.body, reach.trim(), rules.trim());
    let usable: Vec<&str> = file
        .skills
        .iter()
        .filter(|name| {
            skills
                .iter()
                .any(|skill| skill.name == **name && skill.enabled && skill.problem.is_none())
        })
        .map(String::as_str)
        .collect();
    if !usable.is_empty() && may_use(identity, "load_skill") {
        prompt.push_str(&format!(
            "\n\nSkills made for this work; load the one that fits with load_skill before \
             you start: {}.",
            usable.join(", ")
        ));
    }
    Ok(prompt)
}

fn fingerprint(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{catalog::graph::SKILL_FILE, catalog::skills::SkillKind, tools::ToolRegistry};

    fn identity(links: &[&str], tools: Option<&[&str]>) -> Identity {
        Identity {
            name: "tester".into(),
            path: String::new(),
            version: 1,
            content_hash: String::new(),
            description: String::new(),
            entry: true,
            links: links.iter().map(|link| (*link).to_owned()).collect(),
            tools: tools.map(|tools| tools.iter().map(|tool| (*tool).to_owned()).collect()),
            updated_at: 0,
        }
    }

    fn skill(name: &str, enabled: bool) -> SkillInfo {
        SkillInfo {
            name: name.into(),
            description: String::new(),
            kind: SkillKind::Builtin,
            source: None,
            tags: Vec::new(),
            enabled,
            problem: None,
        }
    }

    fn file() -> AgentFile {
        AgentFile {
            body: "You test.".into(),
            skills: vec!["on".into(), "off".into(), "gone".into()],
        }
    }

    #[test]
    fn a_prompt_names_only_skills_that_can_be_loaded() {
        let skills = [skill("on", true), skill("off", false)];
        let prompt = compose(&identity(&[], None), &file(), &skills).unwrap();
        assert!(prompt.starts_with("You test."));
        assert!(prompt.contains("Status:") && prompt.contains("load_skill before"));
        assert!(prompt.ends_with(": on."), "{prompt}");
    }

    #[test]
    fn what_an_agent_is_told_follows_its_tools_and_links() {
        let skills = [skill("on", true)];
        let lead = compose(&identity(&["other"], None), &file(), &skills).unwrap();
        assert!(lead.contains("Use ask_agent"));
        let alone = compose(&identity(&[], None), &file(), &skills).unwrap();
        assert!(alone.contains("You work alone"));
        let limited = identity(&["other"], Some(&["read_file"]));
        let limited = compose(&limited, &file(), &skills).unwrap();
        assert!(limited.contains("You work alone") && !limited.contains("load_skill"));
    }

    /// Every shipped agent reads, names real tools and loads at least some skills. A link
    /// or skill not written yet is skipped at run time, so it is no failure here.
    #[test]
    fn the_shipped_agents_read_and_name_real_tools() {
        let root = resources::root().unwrap();
        let agents = Graph::load(&root.join("agents"), AGENT_FILE);
        let skills = Graph::load(&root.join("skills"), SKILL_FILE);
        assert!(agents.problems().is_empty(), "{:?}", agents.problems());
        let mut known: BTreeSet<String> = ToolRegistry::builtin(&root.join("skills"))
            .names()
            .map(str::to_owned)
            .collect();
        known.extend(["ask_agent", "jobs", "read_history", "tasks"].map(str::to_owned));
        let mut named = 0;
        for agent in agents.nodes() {
            for tool in agent.tools.iter().flatten() {
                assert!(known.contains(tool), "{}: tool {tool}", agent.name);
            }
            let text = read_text(agent.file()).unwrap();
            named += front_skills(&text)
                .iter()
                .filter(|skill| skills.get(skill).is_some())
                .count();
        }
        assert!(named > 0, "the skills were not read");
    }
}
