//! A user message the thread can go back to.

use serde::{Deserialize, Serialize};

/// A user message the thread can go back to.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoPoint {
    pub sequence: u64,
    pub text: String,
}
