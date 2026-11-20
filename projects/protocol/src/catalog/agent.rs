//! Agents as a tree.

use serde::{Deserialize, Serialize};

use crate::catalog::Problem;
/// One agent in the tree a client draws: the chat at the root, then whom each may ask.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentNode {
    pub name: String,
    pub description: String,
    /// `None` for the chat and the built-in general agent, which have no AGENT.md.
    pub version: Option<i64>,
    /// Its own switch; the chat is always on.
    pub enabled: bool,
    /// The agents it may ask, drawn under it or elsewhere in the tree.
    pub links: Vec<String>,
    pub children: Vec<AgentNode>,
    /// Where it came from when the Workshop installed it from a source nobody vouches
    /// for: `<source>, unverified`, as a skill's `source` says it. `None` otherwise.
    #[serde(default)]
    pub source: Option<String>,
}

impl AgentNode {
    /// What every client shows under its name: where it came from, when it is not the
    /// user's own or duskcode's, then what it is for.
    pub fn about(&self) -> String {
        match &self.source {
            Some(source) => format!("from {source} \u{b7} {}", self.description),
            None => self.description.clone(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTree {
    pub root: AgentNode,
    /// Agents that could not be read.
    pub problems: Vec<Problem>,
    /// Links to agents that are not there; skipped, the rest works.
    pub skipped: Vec<Problem>,
}
