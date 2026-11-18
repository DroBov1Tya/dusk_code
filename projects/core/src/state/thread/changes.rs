//! The change ledger: each file a thread's tool calls changed in its project, with the
//! turn and call that changed it and a diff when one was known.

use anyhow::{Context, Result, bail};
use dusk_protocol::project::{EditKind, FileChange, MAX_FILE_CHANGES};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use crate::project::ledger::Edit;
use crate::state::{State, timestamp_ms};

/// Which rows the `changes` tool asks for; every filter but the project is optional.
pub struct ChangeQuery<'a> {
    /// The canonical project folder; rows of other projects are never read.
    pub project: &'a str,
    pub thread_id: Option<&'a str>,
    /// A file, or a folder whose files are wanted, relative to the project.
    pub path: Option<&'a str>,
    /// Unix milliseconds; older rows are left out.
    pub since: Option<i64>,
    pub limit: usize,
}

/// The rows `$filter` picks, each with its turn's number in its thread.
macro_rules! select {
    ($filter:literal) => {
        concat!(
            "SELECT c.*, CASE WHEN c.turn_id IS NULL THEN NULL ELSE
                (SELECT COUNT(*) FROM turns t WHERE t.thread_id = c.thread_id
                    AND t.rowid <= (SELECT rowid FROM turns WHERE turn_id = c.turn_id))
            END AS turn_number FROM changes c ",
            $filter
        )
    };
}

impl State {
    /// Records what one call changed, in the thread's running turn when there is one.
    pub async fn record_changes(
        &self,
        thread_id: &str,
        call_id: Option<&str>,
        project: &str,
        edits: &[Edit],
    ) -> Result<()> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin the changes")?;
        for edit in edits {
            sqlx::query(
                "INSERT INTO changes (change_id, thread_id, turn_id, call_id, project, path,
                    kind, size_before, size_after, hash_after, diff, diff_cut, created_at)
                 VALUES (?, ?, (SELECT turn_id FROM turns WHERE thread_id = ?
                    AND status = 'running'), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(thread_id)
            .bind(thread_id)
            .bind(call_id)
            .bind(project)
            .bind(&edit.path)
            .bind(kind_name(edit.kind))
            .bind(edit.size_before.map(size).transpose()?)
            .bind(edit.size_after.map(size).transpose()?)
            .bind(edit.hash_after.as_deref())
            .bind(edit.diff.as_ref().map(|diff| diff.text.as_str()))
            .bind(edit.diff.as_ref().is_some_and(|diff| diff.cut))
            .bind(now)
            .execute(&mut *transaction)
            .await
            .context("record a change")?;
        }
        transaction.commit().await.context("commit the changes")
    }

    /// The project's changes that pass the filters, newest first.
    pub async fn changes(&self, query: &ChangeQuery<'_>) -> Result<Vec<FileChange>> {
        let limit = i64::try_from(query.limit).context("limit is too large")?;
        sqlx::query(select!(
            "WHERE c.project = ?1
                AND (?2 IS NULL OR c.thread_id = ?2)
                AND (?3 IS NULL OR c.path = ?3 OR substr(c.path, 1, length(?3) + 1) = ?3 || '/')
                AND (?4 IS NULL OR c.created_at >= ?4)
             ORDER BY c.created_at DESC, c.rowid DESC LIMIT ?5"
        ))
        .bind(query.project)
        .bind(query.thread_id)
        .bind(query.path)
        .bind(query.since)
        .bind(limit)
        .try_map(|row: SqliteRow| from_row(&row).map_err(|error| sqlx::Error::Decode(error.into())))
        .fetch_all(&self.pool)
        .await
        .context("list the changes")
    }

    /// The thread's own changes for a client, newest first, at most `MAX_FILE_CHANGES`.
    pub async fn thread_changes(&self, thread_id: &str) -> Result<Vec<FileChange>> {
        sqlx::query(select!(
            "WHERE c.thread_id = ? ORDER BY c.created_at DESC, c.rowid DESC LIMIT ?"
        ))
        .bind(thread_id)
        .bind(MAX_FILE_CHANGES as i64)
        .try_map(|row: SqliteRow| from_row(&row).map_err(|error| sqlx::Error::Decode(error.into())))
        .fetch_all(&self.pool)
        .await
        .context("list the thread's changes")
    }
}

fn size(bytes: u64) -> Result<i64> {
    i64::try_from(bytes).context("file size is too large")
}

fn kind_name(kind: EditKind) -> &'static str {
    match kind {
        EditKind::Created => "created",
        EditKind::Modified => "modified",
        EditKind::Deleted => "deleted",
    }
}

fn from_row(row: &SqliteRow) -> Result<FileChange> {
    let kind = match row.try_get::<&str, _>("kind")? {
        "created" => EditKind::Created,
        "modified" => EditKind::Modified,
        "deleted" => EditKind::Deleted,
        other => bail!("unknown change kind {other:?}"),
    };
    let size = |column: &str| -> Result<Option<u64>> {
        row.try_get::<Option<i64>, _>(column)?
            .map(u64::try_from)
            .transpose()
            .context("negative file size")
    };
    Ok(FileChange {
        change_id: row.try_get("change_id")?,
        thread_id: row.try_get("thread_id")?,
        turn_id: row.try_get("turn_id")?,
        turn: row
            .try_get::<Option<i64>, _>("turn_number")?
            .and_then(|n| u32::try_from(n).ok()),
        call_id: row.try_get("call_id")?,
        path: row.try_get("path")?,
        kind,
        size_before: size("size_before")?,
        size_after: size("size_after")?,
        hash_after: row.try_get("hash_after")?,
        diff: row.try_get("diff")?,
        diff_cut: row.try_get("diff_cut")?,
        at: row.try_get("created_at")?,
    })
}
