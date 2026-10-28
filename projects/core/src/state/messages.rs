use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::events::{Event, MessageCreated};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
    Agent,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
            Self::Agent => "agent",
        }
    }
}

/// A file reference, not a snapshot. Relative paths are interpreted against the message's cwd.
/// Database encryption protects this metadata, not the referenced file's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub path: String,
    pub display_name: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInput {
    pub thread_id: String,
    pub job_id: Option<String>,
    pub role: MessageRole,
    pub agent_id: Option<String>,
    pub content: String,
    /// Model thinking; only assistant messages carry it.
    pub reasoning: Option<String>,
    /// Historical working-directory context; it need not still exist when history is read.
    pub cwd: String,
    pub attachments: Vec<MessageAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message_id: String,
    pub thread_id: String,
    pub job_id: Option<String>,
    pub request_uid: Option<String>,
    /// Stable, increasing order within a thread.
    pub sequence: u64,
    pub role: MessageRole,
    pub agent_id: Option<String>,
    pub content: String,
    /// Model thinking; `None` when not recorded.
    pub reasoning: Option<String>,
    pub cwd: String,
    pub attachments: Vec<MessageAttachment>,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

impl State {
    /// Appends a message under a caller-assigned ID.
    /// Request-linked user messages are appended atomically by the transport.
    /// Receiver-owned ID prefixes are reserved for transactional user turns and responses.
    pub async fn append_message(&self, message_id: &str, input: MessageInput) -> Result<Message> {
        ensure!(
            !message_id.starts_with("request:") && !message_id.starts_with("turn-response:"),
            "message ID prefix is reserved for receivers"
        );
        validate_input(message_id, &input)?;
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin message append")?;
        let message = insert_message(&mut transaction, message_id, &input, None, timestamp).await?;
        transaction
            .commit()
            .await
            .context("commit message append")?;
        let _ = self.events.send(Event::MessageCreated(MessageCreated {
            message: message.clone(),
        }));
        Ok(message)
    }

    pub async fn load_message(&self, message_id: &str) -> Result<Option<Message>> {
        sqlx::query("SELECT * FROM messages WHERE message_id = ?")
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await
            .context("load message")?
            .map(decode_message)
            .transpose()
    }

    /// Returns the next page after a sequence; pass zero for the beginning of a thread.
    pub async fn list_messages(
        &self,
        thread_id: &str,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        ensure!(
            (1..=1000).contains(&limit),
            "message limit must be 1..=1000"
        );
        let after_sequence = i64::try_from(after_sequence)
            .context("message sequence exceeds SQLite integer range")?;
        sqlx::query(
            "SELECT * FROM messages WHERE thread_id = ? AND sequence > ? ORDER BY sequence LIMIT ?",
        )
        .bind(thread_id)
        .bind(after_sequence)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("list messages")?
        .into_iter()
        .map(decode_message)
        .collect()
    }
}

pub(super) fn validate_context(cwd: &str, attachments: &[MessageAttachment]) -> Result<()> {
    ensure!(!cwd.trim().is_empty(), "message working directory is empty");
    ensure!(
        !cwd.contains('\0'),
        "message working directory contains NUL"
    );
    for attachment in attachments {
        ensure!(
            !attachment.path.trim().is_empty(),
            "attachment path is empty"
        );
        ensure!(
            !attachment.path.contains('\0'),
            "attachment path contains NUL"
        );
        if let Some(name) = &attachment.display_name {
            ensure!(
                !name.trim().is_empty() && !name.contains('\0'),
                "invalid attachment name"
            );
        }
        if let Some(mime_type) = &attachment.mime_type {
            ensure!(
                !mime_type.trim().is_empty() && !mime_type.contains('\0'),
                "invalid attachment MIME type"
            );
        }
    }
    Ok(())
}

fn validate_input(message_id: &str, input: &MessageInput) -> Result<()> {
    ensure!(!message_id.trim().is_empty(), "message ID is empty");
    ensure!(!input.thread_id.trim().is_empty(), "thread ID is empty");
    ensure!(
        !input.content.trim().is_empty() || !input.attachments.is_empty(),
        "message is empty"
    );
    ensure!(
        input.job_id.as_ref().is_none_or(|id| !id.trim().is_empty()),
        "job ID is empty"
    );
    ensure!(
        input
            .agent_id
            .as_ref()
            .is_none_or(|id| !id.trim().is_empty()),
        "agent ID is empty"
    );
    ensure!(
        input
            .reasoning
            .as_ref()
            .is_none_or(|text| { input.role == MessageRole::Assistant && !text.trim().is_empty() }),
        "only an assistant message can carry reasoning, and it must not be empty"
    );
    validate_context(&input.cwd, &input.attachments)
}

pub(super) async fn insert_message(
    transaction: &mut Transaction<'_, Sqlite>,
    message_id: &str,
    input: &MessageInput,
    request_uid: Option<&str>,
    timestamp: i64,
) -> Result<Message> {
    validate_input(message_id, input)?;
    let attachments =
        serde_json::to_string(&input.attachments).context("encode message attachments")?;
    let row = sqlx::query(
        "INSERT INTO messages (message_id, thread_id, job_id, request_uid, sequence,
            role, agent_id, content, reasoning, cwd, attachments_json, created_at)
         VALUES (?, ?, ?, ?, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = ?),
            ?, ?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(message_id)
    .bind(&input.thread_id)
    .bind(&input.job_id)
    .bind(request_uid)
    .bind(&input.thread_id)
    .bind(input.role.as_str())
    .bind(&input.agent_id)
    .bind(&input.content)
    .bind(&input.reasoning)
    .bind(&input.cwd)
    .bind(attachments)
    .bind(timestamp)
    .fetch_one(&mut **transaction)
    .await
    .context("append message")?;
    sqlx::query("UPDATE threads SET updated_at = MAX(updated_at, ?) WHERE id = ?")
        .bind(timestamp)
        .bind(&input.thread_id)
        .execute(&mut **transaction)
        .await
        .context("update thread message timestamp")?;
    decode_message(row)
}

pub(super) fn decode_message(row: SqliteRow) -> Result<Message> {
    let role: String = row.try_get("role")?;
    let role = match role.as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => MessageRole::Tool,
        "agent" => MessageRole::Agent,
        _ => bail!("invalid stored message role"),
    };
    let sequence: i64 = row.try_get("sequence")?;
    let attachments: String = row.try_get("attachments_json")?;
    Ok(Message {
        message_id: row.try_get("message_id")?,
        thread_id: row.try_get("thread_id")?,
        job_id: row.try_get("job_id")?,
        request_uid: row.try_get("request_uid")?,
        sequence: sequence.try_into().context("negative message sequence")?,
        role,
        agent_id: row.try_get("agent_id")?,
        content: row.try_get("content")?,
        reasoning: row.try_get("reasoning")?,
        cwd: row.try_get("cwd")?,
        attachments: serde_json::from_str(&attachments).context("decode message attachments")?,
        created_at: row.try_get("created_at")?,
    })
}
