//! Thread messages in order: what the system prompt, the user, the model and the tools said.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};
use crate::{artifacts::ImageArtifact, providers::ToolCall};

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
    /// Image cards for clients. Never included in model context or URL detection.
    pub artifacts: Vec<ImageArtifact>,
    /// For a user message another agent sent: the thread it came from.
    pub from_thread_id: Option<String>,
    /// Estimated sizes in model tokens; see `estimate_tokens`.
    pub content_tokens: u64,
    pub tool_calls_tokens: u64,
    pub reasoning_tokens: u64,
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
pub struct NewMessage {
    role: MessageRole,
    content: String,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
    artifacts: Vec<ImageArtifact>,
    from_thread_id: Option<String>,
}

impl Message {
    /// Estimated size of what the model receives of this message.
    pub fn context_tokens(&self) -> u64 {
        self.content_tokens + self.tool_calls_tokens
    }
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
        self.add_message_from(thread_id, request_uid, content, None)
            .await
    }

    /// The same for a message another agent sent; `from` is that agent's thread.
    pub async fn add_message_from(
        &self,
        thread_id: &str,
        request_uid: &str,
        content: &str,
        from: Option<&str>,
    ) -> Result<Message> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        ensure!(!content.trim().is_empty(), "message is empty");
        let timestamp = timestamp_ms()?;
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
        let mut new = NewMessage::user(content);
        new.from_thread_id = from.map(str::to_owned);
        let message = insert_message(
            &mut transaction,
            thread_id,
            &new,
            Some(request_uid),
            timestamp,
        )
        .await?;
        transaction.commit().await.context("commit user message")?;
        Ok(message)
    }

    /// Stores the system prompt a thread starts with.
    pub async fn add_system_message(&self, thread_id: &str, content: &str) -> Result<Message> {
        ensure!(!content.trim().is_empty(), "system prompt is empty");
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin system message")?;
        ensure_thread_exists(&mut transaction, thread_id).await?;
        let new = NewMessage::system(content);
        let message = insert_message(&mut transaction, thread_id, &new, None, timestamp).await?;
        transaction
            .commit()
            .await
            .context("commit system message")?;
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

    /// Messages whose text or tool calls contain `text`, best matches first.
    pub async fn search_messages(
        &self,
        thread_id: &str,
        text: &str,
        limit: u32,
    ) -> Result<Vec<Message>> {
        // The index is built from trigrams, so shorter text matches nothing.
        ensure!(
            text.trim().chars().count() >= 3,
            "search text needs at least 3 characters"
        );
        // A quoted phrase: the text is matched as it is, never read as query syntax.
        let phrase = format!("\"{}\"", text.trim().replace('"', "\"\""));
        sqlx::query(
            "SELECT messages.* FROM messages_search
             JOIN messages ON messages.rowid = messages_search.rowid
             WHERE messages_search MATCH ? AND messages.thread_id = ?
             ORDER BY messages_search.rank LIMIT ?",
        )
        .bind(phrase)
        .bind(thread_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("search messages")?
        .into_iter()
        .map(decode_message)
        .collect()
    }

    /// Messages from sequence `from` on, oldest first.
    pub async fn messages_from(
        &self,
        thread_id: &str,
        from: u64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        sqlx::query(
            "SELECT * FROM messages WHERE thread_id = ? AND sequence >= ?
             ORDER BY sequence LIMIT ?",
        )
        .bind(thread_id)
        .bind(i64::try_from(from).context("sequence is too large")?)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("read messages")?
        .into_iter()
        .map(decode_message)
        .collect()
    }
}

impl NewMessage {
    pub fn system(content: &str) -> Self {
        Self::plain(MessageRole::System, content)
    }

    pub fn user(content: &str) -> Self {
        Self::plain(MessageRole::User, content)
    }

    pub fn assistant(
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
            artifacts: Vec::new(),
            from_thread_id: None,
        }
    }

    pub fn tool(call_id: String, content: String) -> Self {
        Self {
            role: MessageRole::Tool,
            content,
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id),
            artifacts: Vec::new(),
            from_thread_id: None,
        }
    }

    fn plain(role: MessageRole, content: &str) -> Self {
        Self {
            role,
            content: content.to_owned(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
            artifacts: Vec::new(),
            from_thread_id: None,
        }
    }

    pub fn with_artifacts(mut self, artifacts: Vec<ImageArtifact>) -> Self {
        self.artifacts = artifacts;
        self
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
/// estimate: about four bytes of UTF-8 per token.
pub fn estimate_tokens(text: &str) -> u64 {
    const BYTES_PER_TOKEN: usize = 4;
    text.len().div_ceil(BYTES_PER_TOKEN) as u64
}

/// The role markup around each message in a request.
const MESSAGE_OVERHEAD: u64 = 4;

/// Appends a message at the end of the thread inside the caller's transaction.
pub async fn insert_message(
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
    let content_tokens = estimate_tokens(&message.content) + MESSAGE_OVERHEAD;
    let tool_calls_tokens = tool_calls.as_deref().map_or(0, estimate_tokens);
    let reasoning_tokens = message.reasoning.as_deref().map_or(0, estimate_tokens);
    let artifacts = if message.artifacts.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&message.artifacts).context("encode image artifacts")?)
    };
    let row = sqlx::query(
        "INSERT INTO messages (message_id, thread_id, sequence, role, content, reasoning,
            tool_calls_json, tool_call_id, request_uid, content_tokens, tool_calls_tokens,
            reasoning_tokens, created_at, from_thread_id, artifacts_json)
         VALUES (?, ?, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = ?),
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
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
    .bind(i64::try_from(content_tokens).context("message is too large")?)
    .bind(i64::try_from(tool_calls_tokens).context("tool calls are too large")?)
    .bind(i64::try_from(reasoning_tokens).context("reasoning is too large")?)
    .bind(timestamp)
    .bind(&message.from_thread_id)
    .bind(artifacts)
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

pub fn decode_message(row: SqliteRow) -> Result<Message> {
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
    let artifacts: Option<String> = row.try_get("artifacts_json")?;
    let artifacts = match artifacts {
        Some(json) => serde_json::from_str(&json).context("decode image artifacts")?,
        None => Vec::new(),
    };
    Ok(Message {
        message_id: row.try_get("message_id")?,
        thread_id: row.try_get("thread_id")?,
        sequence: sequence.try_into().context("negative message sequence")?,
        role,
        content: row.try_get("content")?,
        reasoning: row.try_get("reasoning")?,
        tool_calls,
        tool_call_id: row.try_get("tool_call_id")?,
        artifacts,
        from_thread_id: row.try_get("from_thread_id")?,
        content_tokens: read_tokens(&row, "content_tokens")?,
        tool_calls_tokens: read_tokens(&row, "tool_calls_tokens")?,
        reasoning_tokens: read_tokens(&row, "reasoning_tokens")?,
        created_at: row.try_get("created_at")?,
    })
}

pub fn read_tokens(row: &SqliteRow, column: &str) -> Result<u64> {
    let tokens: i64 = row.try_get(column)?;
    tokens.try_into().context("negative token count")
}
