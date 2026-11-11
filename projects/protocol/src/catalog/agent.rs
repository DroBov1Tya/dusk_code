//! Agents as a tree.

use serde::{Deserialize, Serialize};

use crate::catalog::Problem;
/// One agent in the tree a client draws: the chat at the root, then whom each may ask.
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTree {
    pub root: AgentNode,
    /// Agents that could not be read.
    pub problems: Vec<Problem>,
    /// Links to agents that are not there; skipped, the rest works.
    pub skipped: Vec<Problem>,
}
