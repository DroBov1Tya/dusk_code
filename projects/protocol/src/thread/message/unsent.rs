//! A message kept back unsent.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalUnsent {
    pub id: String,
    pub thread_id: String,
    pub content: String,
    pub created_at: i64,
}
