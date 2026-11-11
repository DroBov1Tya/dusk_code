//! The task board of a thread.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// In order.
    pub items: Vec<TaskItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    pub item_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// `None` until work on it begins.
    pub started_at: Option<i64>,
    /// Set exactly when it is done.
    pub finished_at: Option<i64>,
}
