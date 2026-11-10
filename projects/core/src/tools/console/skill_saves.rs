//! Notices for skills a command saved. The skill files are listed before and after the
//! command; a new or changed one that reads as a valid skill becomes a notice.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    catalog::{
        graph::{Node, SKILL_FILE},
        skills::{SkillSave, SkillSaveAction},
    },
    tools::ToolScope,
    util::fs::read_regular,
};

/// Deeper folders are not skills; the walk stops there.
const MAX_DEPTH: usize = 8;
const MAX_SKILL_BYTES: u64 = 256 * 1024;

/// The skill files and their size and change time.
pub type Snapshot = HashMap<PathBuf, (u64, Option<SystemTime>)>;

pub async fn snapshot(scope: &ToolScope) -> Option<Snapshot> {
    let folder = scope.skills()?.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut files = Snapshot::new();
        walk(&folder, 0, &mut files);
        files
    })
    .await
    .ok()
}

/// Links are not followed, so a skill folder cannot point the walk elsewhere.
fn walk(folder: &Path, depth: usize, files: &mut Snapshot) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, files);
        } else if kind.is_file()
            && entry.file_name() == SKILL_FILE
            && let Ok(metadata) = entry.metadata()
        {
            files.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
}

/// The first skill the command created or changed, if it reads as one.
pub async fn saved(scope: &ToolScope, before: &Snapshot) -> Option<SkillSave> {
    let after = snapshot(scope).await?;
    let mut changed: Vec<_> = after
        .iter()
        .filter(|(path, stamp)| before.get(*path) != Some(stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.sort();
    for path in changed {
        let Some(name) = scope.skill_name(&path) else {
            continue;
        };
        let Ok(bytes) = read_regular(&path, MAX_SKILL_BYTES) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if Node::check(&text, &name).is_ok() {
            let action = if before.contains_key(&path) {
                SkillSaveAction::Updated
            } else {
                SkillSaveAction::Created
            };
            return Some(SkillSave { name, action });
        }
    }
    None
}
