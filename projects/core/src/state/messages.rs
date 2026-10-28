//! Thread messages in order: what the system prompt, the user, the model and the tools said.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};
use crate::{events::Event, providers::ToolCall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message_id: String,
    pub thread_id: String,
    /// Order within the thread, from 1.
    pub sequence: u64,
    pub role: MessageRole,
    pub content: String,
    /// Model thinking; only assistant messages carry it, and only when it was recorded.
    pub reasoning: Option<String>,
    /// Only assistant messages call tools.
    pub tool_calls: Vec<ToolCall>,
    /// Set exactly on tool messages: the call they answer.
    pub tool_call_id: Option<String>,
    /// Estimated size in model tokens; see `estimate_tokens`.
    pub token_count: u64,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A message about to be stored. The constructors allow only valid combinations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NewMessage {
    role: MessageRole,
    content: String,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
}

impl State {
    /// Stores what the user wrote. Sending the same request UID again returns the stored
    /// message instead of adding a second one; it must carry the same text.
    pub async fn add_user_message(
        &self,
        thread_id: &str,
        request_uid: &str,
        content: &str,
    ) -> Result<Message> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        ensure!(!content.trim().is_empty(), "message is empty");
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin user message")?;
        let existing = sqlx::query("SELECT * FROM messages WHERE request_uid = ?")
            .bind(request_uid)
            .fetch_optional(&mut *transaction)
            .await
            .context("look up request UID")?;
        if let Some(row) = existing {
            let message = decode_message(row)?;
            ensure!(
                message.thread_id == thread_id && message.content == content,
                "request UID already belongs to a different message"
            );
            return Ok(message);
        }
        ensure_thread_exists(&mut transaction, thread_id).await?;
        let new = NewMessage::user(content);
        let message = insert_message(
            &mut transaction,
            thread_id,
            &new,
            Some(request_uid),
            timestamp,
        )
        .await?;
        transaction.commit().await.context("commit user message")?;
        let _ = self.events.send(Event::MessageCreated(message.clone()));
        Ok(message)
    }

    /// Stores the system prompt a thread starts with.
    pub async fn add_system_message(&self, thread_id: &str, content: &str) -> Result<Message> {
        ensure!(!content.trim().is_empty(), "system prompt is empty");
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin system message")?;
        ensure_thread_exists(&mut transaction, thread_id).await?;
        let new = NewMessage::system(content);
        let message = insert_message(&mut transaction, thread_id, &new, None, timestamp).await?;
        transaction
            .commit()
            .await
            .context("commit system message")?;
        let _ = self.events.send(Event::MessageCreated(message.clone()));
        Ok(message)
    }

    /// The whole thread, in order.
    pub async fn list_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        sqlx::query("SELECT * FROM messages WHERE thread_id = ? ORDER BY sequence")
            .bind(thread_id)
            .fetch_all(&self.pool)
            .await
            .context("list messages")?
            .into_iter()
            .map(decode_message)
            .collect()
    }
}

impl NewMessage {
    pub(super) fn system(content: &str) -> Self {
        Self::plain(MessageRole::System, content)
    }

    pub(super) fn user(content: &str) -> Self {
        Self::plain(MessageRole::User, content)
    }

    pub(super) fn assistant(
        content: String,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: MessageRole::Assistant,
            content,
            reasoning,
            tool_calls,
            tool_call_id: None,
        }
    }

    pub(super) fn tool(call_id: String, content: String) -> Self {
        Self {
            role: MessageRole::Tool,
            content,
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id),
        }
    }

    fn plain(role: MessageRole, content: &str) -> Self {
        Self {
            role,
            content: content.to_owned(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

impl MessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// Providers report token usage per request, not per message, so each message carries an
/// estimate: about four bytes of UTF-8 per token, plus the role markup around the message.
pub fn estimate_tokens(text: &str) -> u64 {
    const BYTES_PER_TOKEN: usize = 4;
    const MESSAGE_OVERHEAD: u64 = 4;
    text.len().div_ceil(BYTES_PER_TOKEN) as u64 + MESSAGE_OVERHEAD
}

/// Appends a message at the end of the thread inside the caller's transaction.
pub(super) async fn insert_message(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    message: &NewMessage,
    request_uid: Option<&str>,
    timestamp: i64,
) -> Result<Message> {
    let tool_calls = match message.tool_calls.as_slice() {
        [] => None,
        calls => Some(serde_json::to_string(calls).context("encode tool calls")?),
    };
    let token_count =
        estimate_tokens(&message.content) + tool_calls.as_deref().map_or(0, estimate_tokens);
    let row = sqlx::query(
        "INSERT INTO messages (message_id, thread_id, sequence, role, content, reasoning,
            tool_calls_json, tool_call_id, request_uid, token_count, created_at)
         VALUES (?, ?, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = ?),
            ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(thread_id)
    .bind(thread_id)
    .bind(message.role.as_str())
    .bind(&message.content)
    .bind(&message.reasoning)
    .bind(tool_calls)
    .bind(&message.tool_call_id)
    .bind(request_uid)
    .bind(i64::try_from(token_count).context("message is too large")?)
    .bind(timestamp)
    .fetch_one(&mut **transaction)
    .await
    .context("store message")?;
    decode_message(row)
}

async fn ensure_thread_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM threads WHERE id = ?)")
        .bind(thread_id)
        .fetch_one(&mut **transaction)
        .await
        .context("look up thread")?;
    ensure!(exists, "thread {thread_id} does not exist");
    Ok(())
}

fn decode_message(row: SqliteRow) -> Result<Message> {
    let role: String = row.try_get("role")?;
    let role = match role.as_str() {
        "system" => MessageRole::System,
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => bail!("invalid stored message role"),
    };
    let tool_calls: Option<String> = row.try_get("tool_calls_json")?;
    let tool_calls = match tool_calls {
        Some(json) => serde_json::from_str(&json).context("decode tool calls")?,
        None => Vec::new(),
    };
    let sequence: i64 = row.try_get("sequence")?;
    let token_count: i64 = row.try_get("token_count")?;
    Ok(Message {
        message_id: row.try_get("message_id")?,
        thread_id: row.try_get("thread_id")?,
        sequence: sequence.try_into().context("negative message sequence")?,
        role,
        content: row.try_get("content")?,
        reasoning: row.try_get("reasoning")?,
        tool_calls,
        tool_call_id: row.try_get("tool_call_id")?,
        token_count: token_count.try_into().context("negative token count")?,
        created_at: row.try_get("created_at")?,
    })
}
