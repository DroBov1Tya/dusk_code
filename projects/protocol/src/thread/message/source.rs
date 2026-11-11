//! A file kept among a thread's sources.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub source_id: String,
    pub name: String,
    pub file: String,
    pub bytes: u64,
    pub pinned: bool,
    pub delivered: bool,
}
