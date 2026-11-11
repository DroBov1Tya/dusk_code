//! What the next request of a thread carries, part by part.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReport {
    /// The size of the next request, as the sidebar counts it.
    pub total: u64,
    pub limit: u64,
    pub sections: Vec<ContextSection>,
    /// What each source category contributed, largest first; empty when nothing was
    /// broken down.
    #[serde(default)]
    pub sources: Vec<ContextSource>,
}

/// One source category's share of the request; only categories with tokens are listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSource {
    pub category: String,
    pub tokens: u64,
}

/// A part of the request; its tokens are estimated, the total may be the provider's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSection {
    pub name: String,
    pub tokens: u64,
    /// What it is made of, largest first.
    pub items: Vec<(String, u64)>,
    /// Per-item detail, where the section has it (skills, MCP tools); empty otherwise.
    #[serde(default)]
    pub details: Vec<ContextItemDetail>,
    /// Which source category the section's tokens belong to, for `sources`; empty for
    /// sections that only break down what another section already counted.
    #[serde(default)]
    pub category: String,
}

/// One item's share and how often it is used, so clients can show both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItemDetail {
    pub name: String,
    /// What it is: `skill` or `mcp tool`.
    pub kind: String,
    pub tokens: u64,
    /// Uses across all threads.
    pub uses_total: u64,
    /// Uses in the reported thread; zero when no thread was given.
    pub uses_in_chat: u64,
}
