//! A problem found in a file.

use serde::{Deserialize, Serialize};

/// A node that could not be read, or a link to a node that is not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub node: String,
    pub reason: String,
}
