//! Compactions: summaries that replace the older messages of a thread in the model
//! context. The messages themselves are never removed.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    Message, State,
    messages::{decode_message, estimate_tokens, read_tokens},
    timestamp_ms,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    pub compaction_id: i64,
    pub thread_id: String,
    /// The newest message the summary covers.
    pub through_sequence: u64,
    pub summary: String,
    /// Estimated size of the summary.
    pub summary_tokens: u64,
    pub reason: CompactionReason,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionReason {
    /// The user asked with `/compact`.
    Manual,
    /// The context neared the model window.
    Auto,
}

impl State {
    pub async fn add_compaction(
        &self,
        thread_id: &str,
        through_sequence: u64,
        summary: &str,
        reason: CompactionReason,
    ) -> Result<Compaction> {
        ensure!(!summary.trim().is_empty(), "summary is empty");
        let timestamp = timestamp_ms()?;
        let row = sqlx::query(
            "INSERT INTO compactions (thread_id, through_sequence, summary, summary_tokens,
                reason, created_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(thread_id)
        .bind(i64::try_from(through_sequence).context("sequence is too large")?)
        .bind(summary)
        .bind(i64::try_from(estimate_tokens(summary)).context("summary is too large")?)
        .bind(reason.as_str())
        .bind(timestamp)
        .fetch_one(&self.pool)
        .await
        .context("store compaction")?;
        decode_compaction(row)
    }

    /// The compaction the current context starts from.
    pub async fn latest_compaction(&self, thread_id: &str) -> Result<Option<Compaction>> {
        sqlx::query(
            "SELECT * FROM compactions WHERE thread_id = ?
             ORDER BY compaction_id DESC LIMIT 1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("load compaction")?
        .map(decode_compaction)
        .transpose()
    }

    /// The messages the model context holds after a summary that covers messages up to
    /// `after`: the system messages and everything newer.
    pub async fn context_messages(&self, thread_id: &str, after: u64) -> Result<Vec<Message>> {
        sqlx::query(
            "SELECT * FROM messages WHERE thread_id = ? AND (role = 'system' OR sequence > ?)
             ORDER BY sequence",
        )
        .bind(thread_id)
        .bind(i64::try_from(after).context("sequence is too large")?)
        .fetch_all(&self.pool)
        .await
        .context("list context messages")?
        .into_iter()
        .map(decode_message)
        .collect()
    }
}

impl CompactionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

fn decode_compaction(row: SqliteRow) -> Result<Compaction> {
    let reason: String = row.try_get("reason")?;
    let reason = match reason.as_str() {
        "manual" => CompactionReason::Manual,
        "auto" => CompactionReason::Auto,
        _ => bail!("invalid stored compaction reason"),
    };
    Ok(Compaction {
        compaction_id: row.try_get("compaction_id")?,
        thread_id: row.try_get("thread_id")?,
        through_sequence: read_tokens(&row, "through_sequence")?,
        summary: row.try_get("summary")?,
        summary_tokens: read_tokens(&row, "summary_tokens")?,
        reason,
        created_at: row.try_get("created_at")?,
    })
}
