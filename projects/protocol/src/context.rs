//! What the next request of a thread carries, part by part.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReport {
    /// The size of the next request, as the sidebar counts it.
    pub total: u64,
    pub limit: u64,
    pub sections: Vec<ContextSection>,
}

/// A part of the request; its tokens are estimated, the total may be the provider's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSection {
    pub name: String,
    pub tokens: u64,
    /// What it is made of, largest first.
    pub items: Vec<(String, u64)>,
}
