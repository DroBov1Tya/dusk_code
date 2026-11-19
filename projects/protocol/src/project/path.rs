//! Completions for a path typed in the input.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathEntry {
    pub name: String,
    pub directory: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCompletion {
    /// Matching entries, folders first, then by name.
    pub entries: Vec<PathEntry>,
    /// Reading stopped early: there may be more.
    pub partial: bool,
}
