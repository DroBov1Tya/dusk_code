//! Thread messages in order: what the system prompt, the user, the model and the tools said.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};
use crate::{
    providers::ToolCall,
    state::{artifacts::Card, thread::threads::family},
};

pub use dusk_protocol::thread::{GoalEvaluation, Message, MessageRole};

/// A message about to be stored. The constructors allow only valid combinations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    role: MessageRole,
    content: String,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
    artifacts: Vec<Card>,
    from_thread_id: Option<String>,
    from_job_id: Option<String>,
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
        let mut transaction = self.pool.begin().await.context("begin user message")?;
        let message = message_from(&mut transaction, thread_id, request_uid, content, from).await?;
        transaction.commit().await.context("commit user message")?;
        Ok(message)
    }

    /// Stores the evaluator's verdict on the chat's goal, for the transcript only.
    pub async fn add_goal_verdict(
        &self,
        thread_id: &str,
        evaluation: &GoalEvaluation,
    ) -> Result<Message> {
        let content = serde_json::to_string(evaluation).context("encode goal verdict")?;
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin goal verdict")?;
        ensure_thread_exists(&mut transaction, thread_id).await?;
        let new = NewMessage::plain(MessageRole::Goal, &content);
        let message = insert_message(&mut transaction, thread_id, &new, None, timestamp).await?;
        transaction.commit().await.context("commit goal verdict")?;
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

    /// How many threads and messages the records hold.
    pub async fn counts(&self) -> Result<(u64, u64)> {
        let (threads, messages): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM threads), (SELECT COUNT(*) FROM messages)",
        )
        .fetch_one(&self.pool)
        .await
        .context("count records")?;
        Ok((threads.max(0) as u64, messages.max(0) as u64))
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

    /// The text of one message of the thread, `None` when it has no such message.
    pub async fn message_content(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT content FROM messages WHERE message_id = ? AND thread_id = ?")
            .bind(message_id)
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read a message")
    }

    /// Whether a message of the thread, or of an agent thread below it, carries the card
    /// `card_id`. The id is compared whole, never matched as a pattern.
    pub async fn carries_card(&self, thread_id: &str, card_id: &str) -> Result<bool> {
        sqlx::query_scalar(family!(
            "SELECT EXISTS (SELECT 1 FROM messages m, json_each(m.artifacts_json) card
                WHERE m.thread_id IN (SELECT id FROM family)
                AND json_extract(card.value, '$.id') = ?2)"
        ))
        .bind(thread_id)
        .bind(card_id)
        .fetch_one(&self.pool)
        .await
        .context("look for the card")
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

    /// What the user and the agent said in any chat that contains `text`, best matches
    /// first. Tool calls and subagents' threads are left out: the search is for what a
    /// person remembers reading.
    /// `within` keeps the search to these chats. They are chosen before the limit: cut
    /// from the answer afterwards, a match that went missing would tell of the matches
    /// in other chats that took its place.
    pub async fn search_chats(
        &self,
        text: &str,
        limit: u32,
        within: Option<&HashSet<String>>,
    ) -> Result<Vec<dusk_protocol::thread::Found>> {
        let text = text.trim();
        ensure!(
            text.chars().count() >= 3,
            "search text needs at least 3 characters"
        );
        // A quoted phrase in the text column: never read as query syntax.
        let phrase = format!("content : \"{}\"", text.replace('"', "\"\""));
        // The ids go as one bound value, a JSON list, never into the query's text.
        let within = within
            .map(serde_json::to_string)
            .transpose()
            .context("list the chats to search")?;
        let rows = sqlx::query(
            "SELECT messages.thread_id, messages.sequence, messages.content, threads.title
             FROM messages_search
             JOIN messages ON messages.rowid = messages_search.rowid
             JOIN threads ON threads.id = messages.thread_id
             WHERE messages_search MATCH ?1 AND threads.parent_thread_id IS NULL
               AND messages.role IN ('user', 'assistant')
               AND (?2 IS NULL OR threads.id IN (SELECT value FROM json_each(?2)))
             ORDER BY messages_search.rank LIMIT ?3",
        )
        .bind(phrase)
        .bind(within)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("search chats")?;
        rows.into_iter()
            .map(|row| {
                let content: String = row.try_get("content")?;
                let sequence: i64 = row.try_get("sequence")?;
                Ok(dusk_protocol::thread::Found {
                    thread_id: row.try_get("thread_id")?,
                    title: row.try_get("title")?,
                    sequence: sequence as u64,
                    snippet: snippet(&content, text),
                })
            })
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
    pub fn job(job_id: &str, content: &str) -> Self {
        let mut message = Self::plain(MessageRole::Job, content);
        message.from_job_id = Some(job_id.to_owned());
        message
    }

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
            from_job_id: None,
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
            from_job_id: None,
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
            from_job_id: None,
        }
    }

    pub fn with_artifacts(mut self, artifacts: Vec<Card>) -> Self {
        self.artifacts = artifacts;
        self
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

/// The estimated size of a message whose content is `content`, its markup included.
pub fn content_tokens(content: &str) -> u64 {
    estimate_tokens(content) + MESSAGE_OVERHEAD
}

/// The call each tool result of `messages`, in thread order, answers, by the result's
/// message ID, with the assistant message that made it. Some providers number calls per
/// step, so an ID can come back later; a result takes the newest call before it.
pub fn calls_of(messages: &[Message]) -> HashMap<&str, (&Message, &ToolCall)> {
    let mut made = HashMap::new();
    let mut answered = HashMap::new();
    for message in messages {
        for call in &message.tool_calls {
            made.insert(call.id.as_str(), (message, call));
        }
        let call = message.tool_call_id.as_deref().and_then(|id| made.get(id));
        if let Some(&call) = call.filter(|_| message.role == MessageRole::Tool) {
            answered.insert(message.message_id.as_str(), call);
        }
    }
    answered
}

/// Stores a user's or another agent's message inside the caller's transaction; the
/// request UID makes a second call return the stored message.
pub(super) async fn message_from(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    request_uid: &str,
    content: &str,
    from: Option<&str>,
) -> Result<Message> {
    ensure!(!request_uid.trim().is_empty(), "request UID is empty");
    ensure!(!content.trim().is_empty(), "message is empty");
    let timestamp = timestamp_ms()?;
    let existing = sqlx::query("SELECT * FROM messages WHERE request_uid = ?")
        .bind(request_uid)
        .fetch_optional(&mut **transaction)
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
    ensure_thread_exists(transaction, thread_id).await?;
    let mut new = NewMessage::user(content);
    new.from_thread_id = from.map(str::to_owned);
    insert_message(transaction, thread_id, &new, Some(request_uid), timestamp).await
}

/// Stores a background job's result inside the caller's transaction, once per job.
pub(super) async fn job_message(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    job_id: &str,
    content: &str,
) -> Result<Message> {
    let existing = sqlx::query("SELECT * FROM messages WHERE from_job_id = ?")
        .bind(job_id)
        .fetch_optional(&mut **transaction)
        .await
        .context("look up job result")?;
    if let Some(row) = existing {
        let message = decode_message(row)?;
        ensure!(
            message.thread_id == thread_id && message.content == content,
            "job completion belongs to a different message"
        );
        return Ok(message);
    }
    let new = NewMessage::job(job_id, content);
    insert_message(transaction, thread_id, &new, None, timestamp_ms()?).await
}

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
    // The model never reads a verdict, so it takes no room in the context.
    let content_tokens = match message.role {
        MessageRole::Goal => 0,
        _ => content_tokens(&message.content),
    };
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
            reasoning_tokens, created_at, from_thread_id, artifacts_json, from_job_id,
            goal_verdict)
         VALUES (?, ?, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = ?),
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
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
    .bind(&message.from_job_id)
    .bind(message.role == MessageRole::Goal)
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
    let from_job_id: Option<String> = row.try_get("from_job_id")?;
    let goal_verdict: bool = row.try_get("goal_verdict")?;
    let role = match role.as_str() {
        "system" if from_job_id.is_some() => MessageRole::Job,
        "system" if goal_verdict => MessageRole::Goal,
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
        from_job_id,
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

/// The words around the first place `text` stands in `content`, on one line.
fn snippet(content: &str, text: &str) -> String {
    const BEFORE: usize = 40;
    const SHOWN: usize = 160;
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let chars: Vec<char> = content.chars().collect();
    let folded: Vec<char> = chars.iter().copied().map(lower).collect();
    let needle: Vec<char> = text.chars().map(lower).collect();
    let at = folded
        .windows(needle.len().max(1))
        .position(|window| window == needle)
        .unwrap_or(0);
    let start = at.saturating_sub(BEFORE);
    let mut line: String = chars[start..]
        .iter()
        .take(SHOWN)
        .map(|c| if c.is_whitespace() { ' ' } else { *c })
        .collect();
    if start > 0 {
        line.insert(0, '\u{2026}');
    }
    if start + SHOWN < chars.len() {
        line.push('\u{2026}');
    }
    line
}

#[cfg(test)]
mod card_tests {
    use crate::{
        providers::ToolCall,
        state::{State, TurnAnswer, artifacts::Card},
        testing::temp_state,
    };

    async fn show(state: &State, thread: &str, card: &str) {
        let pending = state.add_pending(thread, "go").await.unwrap();
        state.admit_pending(thread, &pending).await.unwrap();
        let turn = state.start_turn(thread).await.unwrap().unwrap().turn_id;
        let call = ToolCall {
            id: "c".into(),
            name: "display_image".into(),
            arguments: serde_json::json!({}),
        };
        let answer = TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        state
            .record_tool_calls(&turn, answer, vec![call])
            .await
            .unwrap();
        let card = Card {
            id: card.into(),
            ..Card::default()
        };
        state
            .record_tool_result_with_artifacts(&turn, "c", "shown".into(), vec![card])
            .await
            .unwrap();
    }

    // The check that lets a card be served: only a card the thread or an agent below it
    // carries, by its whole id.
    #[tokio::test]
    async fn a_card_is_carried_only_by_its_own_tree_and_whole_id() {
        let state = temp_state("carries_card").await;
        for id in ["chat", "other"] {
            state.create_thread(id, "test").await.unwrap();
        }
        state
            .create_agent_thread("agent", "chat", "general", "agent")
            .await
            .unwrap();
        show(&state, "chat", "aaaa-1111").await;
        show(&state, "agent", "bbbb-2222").await;
        show(&state, "other", "cccc-3333").await;

        assert!(state.carries_card("chat", "aaaa-1111").await.unwrap());
        assert!(state.carries_card("chat", "bbbb-2222").await.unwrap());
        assert!(!state.carries_card("chat", "cccc-3333").await.unwrap());
        assert!(!state.carries_card("agent", "aaaa-1111").await.unwrap());
        for part in ["aaaa", "1111", "aaaa-111", "aaaa-1111 ", "%"] {
            assert!(!state.carries_card("chat", part).await.unwrap(), "{part}");
        }
    }
}
