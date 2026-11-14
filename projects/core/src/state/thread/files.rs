//! The files a thread's commands touched in its project, kept so the list outlives the
//! command and the restart.

use anyhow::{Context, Result};
use dusk_protocol::project::{Change, ChangeKind};
use sqlx::Row;

use crate::state::State;

impl State {
    /// Notes the files a command touched; a later touch replaces the kind.
    pub async fn note_thread_files(&self, thread_id: &str, changes: &[Change]) -> Result<()> {
        for change in changes {
            sqlx::query(
                "INSERT INTO thread_files (thread_id, path, kind) VALUES (?, ?, ?)
                 ON CONFLICT (thread_id, path) DO UPDATE SET kind = excluded.kind",
            )
            .bind(thread_id)
            .bind(&change.path)
            .bind(kind_name(change.kind))
            .execute(&self.pool)
            .await
            .context("save the files of the thread")?;
        }
        Ok(())
    }

    pub async fn thread_files(&self, thread_id: &str) -> Result<Vec<Change>> {
        let rows =
            sqlx::query("SELECT path, kind FROM thread_files WHERE thread_id = ? ORDER BY path")
                .bind(thread_id)
                .fetch_all(&self.pool)
                .await
                .context("list the files of the thread")?;
        Ok(rows
            .iter()
            .map(|row| Change {
                path: row.get("path"),
                kind: kind_from(row.get("kind")),
            })
            .collect())
    }
}

fn kind_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed => "renamed",
        ChangeKind::Untracked => "untracked",
        ChangeKind::Conflicted => "conflicted",
    }
}

fn kind_from(name: &str) -> ChangeKind {
    match name {
        "added" => ChangeKind::Added,
        "deleted" => ChangeKind::Deleted,
        "renamed" => ChangeKind::Renamed,
        "untracked" => ChangeKind::Untracked,
        "conflicted" => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}
