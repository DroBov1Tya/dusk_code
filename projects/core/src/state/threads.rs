use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::{CoreEvent, ThreadCreated, ThreadUpdated};

use super::State;

/// Whether operations may request approval from the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    OnRequest,
    Never,
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnRequest => "on_request",
            Self::Never => "never",
        }
    }
}

/// Mutable metadata. Source and sandbox policy formats remain caller-defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadInput {
    pub source: String,
    /// SQLite stores non-negative signed 64-bit integers; larger counts are rejected.
    pub tokens_used: u64,
    pub compaction_count: u64,
    pub sandbox_policy: String,
    pub approval_mode: ApprovalMode,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_origin_url: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMetadata {
    pub id: String,
    pub source: String,
    /// Unix timestamp in milliseconds, assigned by the core on creation.
    pub created_at: i64,
    /// Unix timestamp in milliseconds, non-decreasing across updates.
    pub updated_at: i64,
    pub tokens_used: u64,
    pub compaction_count: u64,
    pub sandbox_policy: String,
    pub approval_mode: ApprovalMode,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_origin_url: Option<String>,
    pub title: String,
}

impl State {
    /// Inserts a caller-assigned stable ID. Existing IDs are never overwritten.
    pub async fn create_thread(&self, id: &str, input: ThreadInput) -> Result<ThreadMetadata> {
        let (tokens_used, compaction_count) = validate_input(id, &input)?;
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin thread creation")?;
        let row = sqlx::query(
            "INSERT INTO threads
               (id, source, created_at, updated_at, tokens_used, compaction_count,
                sandbox_policy, approval_mode, git_branch, git_commit_sha, git_origin_url, title)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(id)
        .bind(input.source)
        .bind(timestamp)
        .bind(timestamp)
        .bind(tokens_used)
        .bind(compaction_count)
        .bind(input.sandbox_policy)
        .bind(input.approval_mode.as_str())
        .bind(input.git_branch)
        .bind(input.git_commit_sha)
        .bind(input.git_origin_url)
        .bind(input.title)
        .fetch_one(&mut *transaction)
        .await
        .context("create thread metadata")?;
        let thread = decode_thread(row)?;
        transaction
            .commit()
            .await
            .context("commit thread creation")?;
        // No listeners is normal and does not invalidate a committed write.
        let _ = self.events.send(CoreEvent::ThreadCreated(ThreadCreated {
            thread: thread.clone(),
        }));
        Ok(thread)
    }

    /// Replaces mutable metadata, preserving the stable ID and creation timestamp.
    /// Missing IDs are reported without creating a row or emitting an event.
    pub async fn update_thread(
        &self,
        id: &str,
        input: ThreadInput,
    ) -> Result<Option<ThreadMetadata>> {
        let (tokens_used, compaction_count) = validate_input(id, &input)?;
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin thread update")?;
        let row = sqlx::query(
            "UPDATE threads SET source = ?, updated_at = MAX(updated_at, ?),
               tokens_used = ?, compaction_count = ?, sandbox_policy = ?, approval_mode = ?,
               git_branch = ?, git_commit_sha = ?, git_origin_url = ?, title = ?
               WHERE id = ? RETURNING *",
        )
        .bind(input.source)
        .bind(timestamp)
        .bind(tokens_used)
        .bind(compaction_count)
        .bind(input.sandbox_policy)
        .bind(input.approval_mode.as_str())
        .bind(input.git_branch)
        .bind(input.git_commit_sha)
        .bind(input.git_origin_url)
        .bind(input.title)
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await
        .context("update thread metadata")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let thread = decode_thread(row)?;
        transaction.commit().await.context("commit thread update")?;
        let _ = self.events.send(CoreEvent::ThreadUpdated(ThreadUpdated {
            thread: thread.clone(),
        }));
        Ok(Some(thread))
    }

    pub async fn load_thread(&self, id: &str) -> Result<Option<ThreadMetadata>> {
        sqlx::query("SELECT * FROM threads WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("load thread metadata")?
            .map(decode_thread)
            .transpose()
    }
}

fn validate_input(id: &str, input: &ThreadInput) -> Result<(i64, i64)> {
    ensure!(!id.trim().is_empty(), "thread ID is empty");
    ensure!(!input.source.trim().is_empty(), "thread source is empty");
    ensure!(
        !input.sandbox_policy.trim().is_empty(),
        "thread sandbox policy is empty"
    );
    let tokens_used =
        i64::try_from(input.tokens_used).context("tokens used exceeds SQLite integer range")?;
    let compaction_count = i64::try_from(input.compaction_count)
        .context("compaction count exceeds SQLite integer range")?;
    Ok((tokens_used, compaction_count))
}

fn timestamp_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?;
    i64::try_from(elapsed.as_millis()).context("timestamp exceeds SQLite integer range")
}

fn decode_thread(row: SqliteRow) -> Result<ThreadMetadata> {
    let tokens_used: i64 = row.try_get("tokens_used")?;
    let compaction_count: i64 = row.try_get("compaction_count")?;
    let approval_mode: String = row.try_get("approval_mode")?;
    let approval_mode = match approval_mode.as_str() {
        "on_request" => ApprovalMode::OnRequest,
        "never" => ApprovalMode::Never,
        _ => bail!("invalid stored approval mode"),
    };
    Ok(ThreadMetadata {
        id: row.try_get("id")?,
        source: row.try_get("source")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        tokens_used: tokens_used.try_into().context("negative tokens used")?,
        compaction_count: compaction_count
            .try_into()
            .context("negative compaction count")?,
        sandbox_policy: row.try_get("sandbox_policy")?,
        approval_mode,
        git_branch: row.try_get("git_branch")?,
        git_commit_sha: row.try_get("git_commit_sha")?,
        git_origin_url: row.try_get("git_origin_url")?,
        title: row.try_get("title")?,
    })
}
