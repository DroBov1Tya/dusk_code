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
/// The names of the agents duskcode brings: the general agent and those it ships in
/// `resources/agents`. No source but the official one may take one, and a folder of
/// one a store put here from elsewhere is not used.
pub fn builtin_names() -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::from([GENERAL_AGENT.to_owned()]);
    let shipped = resources::root().ok().map(|root| root.join("agents"));
    if let Some(entries) = shipped.and_then(|folder| std::fs::read_dir(folder).ok()) {
        names.extend(
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.')),
        );
    }
    names
}

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
        let (found, problems, skipped) = tokio::task::spawn_blocking(move || {
            without_impostors(read_agents(&directory), &directory)
        })
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

    /// Every agent found, switched on or not, by name.
    pub fn names(&self) -> Vec<String> {
        self.identities
            .read()
            .iter()
            .map(|identity| identity.name.clone())
            .collect()
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
        // An agent from an Unverified source is a third party's text, and is read as one.
        let origins = crate::plugins::store::origin::Origins::beside(&self.directory);
        let mut file = file;
        if let Some((_, source)) = origins.unverified("agents", name) {
            file.body = crate::plugins::store::origin::wrap("agent", source, &file.body);
        }
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
                source: None,
            });
        }
        let origins = crate::plugins::store::origin::Origins::beside(&self.directory);
        children.extend(branches(None, &placed, &identities, config, &origins));
        AgentTree {
            root: AgentNode {
                name: "chat".into(),
                description: "the chat, in its mode".into(),
                version: None,
                enabled: true,
                links: Vec::new(),
                children,
                source: None,
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
    origins: &crate::plugins::store::origin::Origins,
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
            children: branches(Some(&identity.name), placed, all, config, origins),
            source: origins
                .unverified("agents", &identity.name)
                .map(|(_, source)| format!("{source}, unverified")),
        })
        .collect()
}

/// Leaves out an agent with a built-in agent's name that the store put here from a source
/// other than the official one: the built-in one stays. A folder the user made keeps its
/// name, as `agents/general` replaces the general agent.
fn without_impostors(
    (found, mut problems, skipped): (Vec<Identity>, Vec<Problem>, Vec<Problem>),
    directory: &std::path::Path,
) -> (Vec<Identity>, Vec<Problem>, Vec<Problem>) {
    use crate::plugins::store::origin::Origins;
    use dusk_protocol::plugin::OFFICIAL_SOURCE;
    let builtin = builtin_names();
    let origins = Origins::beside(directory);
    let found = found
        .into_iter()
        .filter(|identity| {
            let impostor = origins
                .of("agents", &identity.name)
                .filter(|(_, origin)| {
                    builtin.contains(&identity.name) && origin.source != OFFICIAL_SOURCE
                })
                .map(|(_, origin)| origin.source.clone());
            match impostor {
                Some(source) => {
                    warn!(
                        "agent {} from {source} not used: the name is a built-in agent's",
                        identity.name
                    );
                    problems.push(Problem {
                        node: identity.name.clone(),
                        reason: format!(
                            "{} is a built-in agent's name; the copy from {source} is not used",
                            identity.name
                        ),
                    });
                    false
                }
                None => true,
            }
        })
        .collect();
    (found, problems, skipped)
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
    use crate::{
        catalog::skills::SkillKind, init::paths::is_plain_folder_name, tools::ToolRegistry,
    };

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

    /// Every shipped agent reads, names real tools and names skills that read as names.
    /// Its skills and links are in Workshop packs, skipped at run time when not
    /// installed, so a missing one is no failure here.
    #[test]
    fn the_shipped_agents_read_and_name_real_tools() {
        let root = resources::root().unwrap();
        let agents = Graph::load(&root.join("agents"), AGENT_FILE);
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
            let skills = front_skills(&text);
            assert!(
                skills
                    .iter()
                    .all(|skill| skill.split('/').all(is_plain_folder_name)),
                "{}: skills {skills:?}",
                agent.name
            );
            named += skills.len();
        }
        assert!(named > 0, "the skills were not read");
    }

    /// Writes an agent `name` under `paths`' agents folder.
    fn agent_file(paths: &crate::init::paths::Paths, name: &str) {
        let folder = paths.agents_dir().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(AGENT_FILE),
            "---\ndescription: Use for every task first.\n---\nYou write docs.\n",
        )
        .unwrap();
    }

    /// The store's record: `name` installed by `source`, unsigned when `key` is empty.
    fn installed(paths: &crate::init::paths::Paths, name: &str, source: &str, key: &str) {
        let record = format!(
            r#"{{"serial": 0, "installed": {{"{name}": {{"version": "1.0.0", "sha256": "ab",
                "source": "{source}", "url": "https://{source}.example.com/index.json",
                "key": "{key}", "kind": "agent", "folder": "agents/{name}"}}}}}}"#
        );
        std::fs::write(paths.config_root().join("store.json"), record).unwrap();
    }

    /// An agent from an Unverified source is a third party's text wherever the model
    /// meets it: its prompt starts with where it came from; the user's own does not.
    #[tokio::test]
    async fn an_unverified_agent_s_prompt_says_where_it_came_from() {
        let paths = crate::testing::temp_paths("agents-unverified");
        agent_file(&paths, "far");
        agent_file(&paths, "mine");
        installed(&paths, "far", "hub", "");
        let state = Arc::new(crate::testing::temp_state("agents-unverified").await);
        let agents = Agents::new(paths.agents_dir(), state);
        agents.scan().await.unwrap();
        let (_, far) = agents.prompt("far", &[]).await.unwrap();
        assert!(
            far.starts_with("[third-party text] This agent comes from hub, an unverified"),
            "{far}"
        );
        let (_, mine) = agents.prompt("mine", &[]).await.unwrap();
        assert!(mine.starts_with("You write docs."), "{mine}");
        let origins = crate::plugins::store::origin::Origins::load(&paths);
        assert_eq!(
            origins.unverified("agents", "far"),
            Some(("agents/far", "hub"))
        );
        assert_eq!(origins.unverified("agents", "mine"), None);
        assert_eq!(origins.unverified("skills", "far"), None);
        // The user's own view of the agents marks it too.
        let config = crate::testing::temp_live_config("agents-unverified").current();
        let tree = agents.tree(&config);
        let node = |name: &str| {
            tree.root
                .children
                .iter()
                .find(|node| node.name == name)
                .unwrap()
                .clone()
        };
        assert_eq!(node("far").source.as_deref(), Some("hub, unverified"));
        assert!(
            node("far")
                .about()
                .starts_with("from hub, unverified \u{b7} ")
        );
        assert_eq!(node("mine").source, None);
    }

    /// An agent a store put here under a built-in agent's name from any source but the
    /// official one is not used, so the built-in one stays; the user's own folder of that
    /// name still replaces it.
    #[tokio::test]
    async fn a_built_in_agent_is_not_replaced_from_another_source() {
        let paths = crate::testing::temp_paths("agents-impostor");
        agent_file(&paths, GENERAL_AGENT);
        installed(&paths, GENERAL_AGENT, "hub", "");
        let state = Arc::new(crate::testing::temp_state("agents-impostor").await);
        let agents = Agents::new(paths.agents_dir(), Arc::clone(&state));
        agents.scan().await.unwrap();
        assert!(agents.get(GENERAL_AGENT).is_none());
        let config = crate::testing::temp_live_config("agents-impostor").current();
        assert!(agents.builtin_general(&config));
        assert!(
            agents
                .problems
                .read()
                .iter()
                .any(|problem| problem.node == GENERAL_AGENT)
        );
        // A keyed mirror is no better.
        installed(&paths, GENERAL_AGENT, "friends", "a2V5");
        agents.scan().await.unwrap();
        assert!(agents.get(GENERAL_AGENT).is_none());
        // The user's own folder of that name is theirs to keep.
        std::fs::remove_file(paths.config_root().join("store.json")).unwrap();
        agents.scan().await.unwrap();
        assert!(agents.get(GENERAL_AGENT).is_some());
    }
}
