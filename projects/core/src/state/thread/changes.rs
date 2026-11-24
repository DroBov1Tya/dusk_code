//! The change ledger: each file a thread's tool calls changed in its project, with the
//! turn and call that changed it and a diff when one was known.

use anyhow::{Context, Result, bail};
use dusk_protocol::project::{EditKind, FileChange, MAX_FILE_CHANGES};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use crate::project::ledger::Edit;
use crate::state::{State, timestamp_ms};

/// Calls whose changes went unrecorded, kept for their turns to read.
const MAX_UNWATCHED: usize = 64;

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

/// A file's state after a change: its content hash, `deleted`, or the row's own id when
/// the content was not read, so a state not known is never taken for one seen before.
macro_rules! state_after {
    () => {
        "COALESCE(hash_after, CASE kind WHEN 'deleted' THEN 'deleted' ELSE change_id END)"
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

    /// The paths the turns of `member` on `task_id` changed that only its own walks saw:
    /// a path another thread recorded in the same project since the task's first turn
    /// is left out. With members at work at once, one member's walk around its command
    /// also sees what another wrote meanwhile, and that one records it too, so whose
    /// change it was cannot be told; a path only this member saw changed during one of
    /// its own commands. Sorted, each once.
    pub async fn task_own_changes(&self, member: &str, task_id: &str) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT DISTINCT c.path FROM changes c JOIN turns t ON t.turn_id = c.turn_id
             WHERE c.thread_id = ?1 AND t.task_id = ?2
               AND NOT EXISTS (SELECT 1 FROM changes o
                   WHERE o.project = c.project AND o.path = c.path AND o.thread_id != ?1
                     AND o.created_at >= (SELECT MIN(started_at) FROM turns
                         WHERE thread_id = ?1 AND task_id = ?2))
             ORDER BY c.path",
        )
        .bind(member)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .context("list what the task's turns changed")
    }

    /// Notes that the call `call_id` of the thread may have changed project files the
    /// ledger did not record: the walk around its command gave up (a project or a change
    /// past the walk's caps, or out of time), the recording failed, or the command went
    /// to the background. Only the newest marks are kept.
    pub fn note_unwatched(&self, thread_id: &str, call_id: &str) {
        let mut marks = self.unwatched.lock().unwrap_or_else(|e| e.into_inner());
        if marks.len() >= MAX_UNWATCHED {
            marks.pop_front();
        }
        marks.push_back((thread_id.to_owned(), call_id.to_owned()));
    }

    /// Whether `note_unwatched` marked the call; the mark is taken.
    pub fn take_unwatched(&self, thread_id: &str, call_id: &str) -> bool {
        let mut marks = self.unwatched.lock().unwrap_or_else(|e| e.into_inner());
        let at = marks
            .iter()
            .position(|(thread, call)| thread == thread_id && call == call_id);
        at.and_then(|at| marks.remove(at)).is_some()
    }

    /// How many changes the ledger holds for the thread. Rows are only added, so two
    /// counts tell whether a project file changed between them.
    pub async fn change_count(&self, thread_id: &str) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM changes WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .context("count the thread's changes")?;
        u64::try_from(count).context("negative change count")
    }

    /// What the call `call_id` of the thread changed, as (path, state after), in the
    /// order recorded; see `state_after!`.
    pub async fn call_changes(
        &self,
        thread_id: &str,
        call_id: &str,
    ) -> Result<Vec<(String, String)>> {
        sqlx::query_as(concat!(
            "SELECT path, ",
            state_after!(),
            " FROM changes WHERE thread_id = ? AND call_id = ? ORDER BY rowid"
        ))
        .bind(thread_id)
        .bind(call_id)
        .fetch_all(&self.pool)
        .await
        .context("list what a call changed")
    }

    /// Each file the threads changed with its latest state, as (path, state after), by
    /// path; see `state_after!`.
    pub async fn files_state(&self, threads: &[String]) -> Result<Vec<(String, String)>> {
        // SQLite takes the other columns of a MAX() group from the row with the maximum.
        sqlx::query_as(concat!(
            "SELECT path, state FROM (SELECT path, ",
            state_after!(),
            " AS state, MAX(rowid) FROM changes
                WHERE thread_id IN (SELECT value FROM json_each(?)) GROUP BY path)
             ORDER BY path"
        ))
        .bind(serde_json::to_string(threads)?)
        .fetch_all(&self.pool)
        .await
        .context("list the state of the changed files")
    }

    /// Whether the threads recorded a change since the turn `turn_id` started; false
    /// for a turn not known.
    pub async fn changed_in_turn(&self, threads: &[String], turn_id: &str) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM changes
                WHERE thread_id IN (SELECT value FROM json_each(?1))
                  AND created_at >= (SELECT started_at FROM turns WHERE turn_id = ?2))",
        )
        .bind(serde_json::to_string(threads)?)
        .bind(turn_id)
        .fetch_one(&self.pool)
        .await
        .context("tell whether a turn changed a file")
    }

    /// Each change the turns of `member` on `task_id` recorded, oldest first, as its path
    /// and its diff when one was known: what a reviewer of the task reads. Secret and
    /// unknown files have no diff.
    pub async fn task_diffs(
        &self,
        member: &str,
        task_id: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        sqlx::query_as(
            "SELECT c.path, c.diff FROM changes c JOIN turns t ON t.turn_id = c.turn_id
             WHERE c.thread_id = ?1 AND t.task_id = ?2
             ORDER BY c.created_at, c.rowid",
        )
        .bind(member)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .context("read the diffs of the task's turns")
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
