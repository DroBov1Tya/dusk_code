//! The text of a catalog entry.

use serde::{Deserialize, Serialize};

/// A skill's or an agent's file as a client edits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeText {
    pub name: String,
    /// The front matter fields with plain values, by name, such as `description` and
    /// `license`.
    pub metadata: Vec<(String, String)>,
    /// The whole file, front matter included.
    pub text: String,
}
