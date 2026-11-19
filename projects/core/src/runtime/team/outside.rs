//! What a member changed outside its task's files, from the change ledger. The ledger
//! walks the project around each command with the ignore rules, so build output and
//! other ignored paths never show. A change it cannot tell from another member's is
//! left out (`State::task_own_changes`): the check holds what is surely the member's.

use anyhow::Result;

use super::lead_lines;
use crate::{
    runtime::Launcher,
    state::{PlanTask, overlaps},
};

/// Paths named in the line to the lead, and their most characters together.
const SHOWN: usize = 4;
const SHOWN_CHARS: usize = 160;

/// `changed outside its files: src/x.rs, notes.md and 2 more`, or `None` when every
/// change of the task's turns is inside its files. A task without files names none.
pub(super) async fn line(
    launcher: &Launcher,
    member: &str,
    task: &PlanTask,
) -> Result<Option<String>> {
    let files = task.files.entries();
    if files.is_empty() {
        return Ok(None);
    }
    let changed = launcher
        .state
        .task_own_changes(member, &task.task_id)
        .await?;
    let outside: Vec<&String> = changed
        .iter()
        .filter(|path| !files.iter().any(|entry| overlaps(entry, path)))
        .collect();
    if outside.is_empty() {
        return Ok(None);
    }
    let mut shown: Vec<String> = Vec::new();
    let mut length = 0;
    for path in outside.iter().take(SHOWN) {
        let path = lead_lines::quoted(path, 80);
        if !shown.is_empty() && length + path.len() > SHOWN_CHARS {
            break;
        }
        length += path.len() + 2;
        shown.push(path);
    }
    let more = match outside.len() - shown.len() {
        0 => String::new(),
        left => format!(" and {left} more"),
    };
    Ok(Some(format!(
        "changed outside its files: {}{more}",
        shown.join(", ")
    )))
}
