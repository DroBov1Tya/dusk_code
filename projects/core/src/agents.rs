//! The agent graph. Every folder under `agents/` with an `AGENT.md` is an expert agent:
//! its text is the system prompt of the threads it works in. The folder is read at the
//! start and then every minute, and what is found is recorded as identities, with a
//! version that grows when a file changes. A chat asks the entry agents; an agent asks
//! the agents it links to. The built-in general agent may be asked from anywhere.
//! Switching an agent off in the config hides it from every thread.

use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use tracing::warn;

use crate::{
    config::Config,
    graph::{AGENT_FILE, Graph, Problem, read_text, split_front_matter},
    state::{Identity, State},
    sync::{read, write},
    thread_start::fnv1a,
};

/// The built-in agent, asked when none is named. An `agents/general` folder replaces it.
pub const GENERAL_AGENT: &str = "general";
pub const GENERAL_DESCRIPTION: &str = "works like this chat, with MAIN.md and every tool; \
for research, reading code and self-contained parts of a task";
/// A chat is at depth 0; an agent three levels down can no longer ask another.
pub const MAX_AGENT_DEPTH: usize = 3;

pub struct Agents {
    directory: PathBuf,
    state: Arc<State>,
    identities: RwLock<Vec<Identity>>,
    problems: RwLock<Vec<Problem>>,
}

/// One agent in the tree a client draws: the chat at the root, then whom each may ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNode {
    pub name: String,
    pub description: String,
    /// `None` for the chat and the built-in general agent, which have no AGENT.md.
    pub version: Option<i64>,
    /// Its own switch; the chat is always on.
    pub enabled: bool,
    pub children: Vec<AgentNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTree {
    pub root: AgentNode,
    /// Agents that could not be read, and links to agents that do not exist.
    pub problems: Vec<Problem>,
}

impl Agents {
    pub fn new(directory: PathBuf, state: Arc<State>) -> Arc<Self> {
        Arc::new(Self {
            directory,
            state,
            identities: RwLock::default(),
            problems: RwLock::default(),
        })
    }

    /// Reads `agents/` and records what is there.
    pub async fn scan(&self) -> Result<()> {
        let directory = self.directory.clone();
        let (found, problems) = tokio::task::spawn_blocking(move || read_agents(&directory))
            .await
            .context("cannot read the agents")?;
        let identities = self.state.sync_identities(&found).await?;
        *write(&self.identities) = identities;
        *write(&self.problems) = problems;
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
        read(&self.identities)
            .iter()
            .find(|identity| identity.name == name)
            .cloned()
    }

    /// Whom a thread may ask: the entries for a chat (`asker` is `None`), the links for an
    /// agent. Switched-off agents are left out.
    pub fn reachable(&self, asker: Option<&str>, config: &Config) -> Vec<Identity> {
        let identities = read(&self.identities);
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

    /// The agent's prompt, from the file its identity names. A file edited since the last
    /// read is recorded first, so the identity returned is the version the prompt is.
    pub async fn prompt(&self, name: &str) -> Result<(Identity, String)> {
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

    /// The chat and every agent it can reach, as deep as agents may ask each other. An
    /// agent several others link to appears under each of them.
    pub fn tree(&self, config: &Config) -> AgentTree {
        let identities = read(&self.identities).clone();
        let mut children = Vec::new();
        if self.get(GENERAL_AGENT).is_none() {
            children.push(AgentNode {
                name: GENERAL_AGENT.into(),
                description: GENERAL_DESCRIPTION.into(),
                version: None,
                enabled: config.agent_enabled(GENERAL_AGENT),
                children: Vec::new(),
            });
        }
        children.extend(
            identities
                .iter()
                .filter(|identity| identity.entry)
                .map(|identity| node(identity, &identities, config, 1)),
        );
        AgentTree {
            root: AgentNode {
                name: "chat".into(),
                description: "MAIN.md".into(),
                version: None,
                enabled: true,
                children,
            },
            problems: read(&self.problems).clone(),
        }
    }
}

fn node(identity: &Identity, all: &[Identity], config: &Config, depth: usize) -> AgentNode {
    let children = if depth < MAX_AGENT_DEPTH {
        identity
            .links
            .iter()
            .filter_map(|link| all.iter().find(|other| other.name == *link))
            .map(|linked| node(linked, all, config, depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    AgentNode {
        name: identity.name.clone(),
        description: identity.description.clone(),
        version: Some(identity.version),
        enabled: config.agent_enabled(&identity.name),
        children,
    }
}

fn read_agents(directory: &std::path::Path) -> (Vec<Identity>, Vec<Problem>) {
    let graph = Graph::load(directory, AGENT_FILE);
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
    (found, problems)
}

/// The prompt after the front matter, and the fingerprint of the whole file.
fn read_prompt(identity: &Identity) -> Result<(String, String)> {
    let text = read_text(std::path::Path::new(&identity.path))
        .with_context(|| format!("cannot read agent {}", identity.name))?;
    let (_, body) = split_front_matter(&text)?;
    Ok((body.trim().to_owned(), fingerprint(text.as_bytes())))
}

fn fingerprint(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}
