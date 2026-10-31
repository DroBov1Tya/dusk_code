//! Threads: one per chat, and one per subagent conversation under the thread that asked
//! it. Everything else in a thread is its messages and turns.

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    /// Who started the thread, such as `cli`.
    pub source: String,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
    /// `None` until the first message.
    pub title: Option<String>,
    /// When the newest message or local unsent input was stored, or `created_at`.
    pub active_at: i64,
    /// For a subagent's thread: the thread that asked it, and the agent's name.
    pub parent_thread_id: Option<String>,
    pub agent: Option<String>,
    /// What an agent's thread goes by, such as `Fable-3a`.
    pub name: Option<String>,
    /// The folder a chat was started in; `None` for one older than this record.
    pub project: Option<String>,
}

impl State {
    pub async fn create_thread(&self, id: &str, source: &str) -> Result<Thread> {
        self.insert_thread(id, source, None, None, None).await
    }

    pub async fn set_thread_project(&self, id: &str, dir: &str) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO thread_projects (thread_id, dir) VALUES (?, ?)")
            .bind(id)
            .bind(dir)
            .execute(&self.pool)
            .await
            .context("store the thread's folder")?;
        Ok(())
    }

    /// A chat older than the record of folders belongs to the first one it is opened in.
    pub async fn adopt_thread(&self, id: &str, dir: &str) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO thread_projects (thread_id, dir) VALUES (?, ?)")
            .bind(id)
            .bind(dir)
            .execute(&self.pool)
            .await
            .context("store the thread's folder")?;
        Ok(())
    }

    /// The chats started in `dir`, and those older than the record of where a chat
    /// started, which would otherwise be out of reach everywhere.
    pub async fn project_threads(&self, dir: &str) -> Result<Vec<Thread>> {
        let mut threads = self.list_threads().await?;
        threads.retain(|thread| {
            thread
                .project
                .as_deref()
                .is_none_or(|project| project == dir)
        });
        Ok(threads)
    }

    /// A thread for `agent` to work in under the thread that asked it, called `name`.
    pub async fn create_agent_thread(
        &self,
        id: &str,
        parent: &str,
        agent: &str,
        name: &str,
    ) -> Result<Thread> {
        self.insert_thread(id, "agent", Some(parent), Some(agent), Some(name))
            .await
    }

    async fn insert_thread(
        &self,
        id: &str,
        source: &str,
        parent: Option<&str>,
        agent: Option<&str>,
        name: Option<&str>,
    ) -> Result<Thread> {
        ensure!(!id.trim().is_empty(), "thread ID is empty");
        ensure!(!source.trim().is_empty(), "thread source is empty");
        let timestamp = timestamp_ms()?;
        sqlx::query(
            "INSERT INTO threads (id, source, created_at, parent_thread_id, agent, name)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(source)
        .bind(timestamp)
        .bind(parent)
        .bind(agent)
        .bind(name)
        .execute(&self.pool)
        .await
        .context("create thread")?;
        self.load_thread(id).await?.context("thread was not stored")
    }

    pub async fn load_thread(&self, id: &str) -> Result<Option<Thread>> {
        sqlx::query(
            "SELECT threads.*, (SELECT dir FROM thread_projects WHERE thread_id = threads.id) AS project,
                MAX(threads.created_at,
                COALESCE((SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id), threads.created_at),
                COALESCE((SELECT MAX(created_at) FROM local_unsent WHERE thread_id = threads.id), threads.created_at)) AS active_at
             FROM threads WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("load thread")?
        .map(decode_thread)
        .transpose()
    }

    /// The threads people chat in, the most recently active first. Subagent threads are
    /// reached through `agent_threads`.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        sqlx::query(
            "SELECT threads.*, (SELECT dir FROM thread_projects WHERE thread_id = threads.id) AS project,
                MAX(threads.created_at,
                COALESCE((SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id), threads.created_at),
                COALESCE((SELECT MAX(created_at) FROM local_unsent WHERE thread_id = threads.id), threads.created_at)) AS active_at
             FROM threads WHERE parent_thread_id IS NULL ORDER BY active_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("list threads")?
        .into_iter()
        .map(decode_thread)
        .collect()
    }

    /// The subagent threads a thread started, oldest first.
    pub async fn agent_threads(&self, parent: &str) -> Result<Vec<Thread>> {
        sqlx::query(
            "SELECT threads.*, (SELECT dir FROM thread_projects WHERE thread_id = threads.id) AS project,
                MAX(threads.created_at,
                COALESCE((SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id), threads.created_at),
                COALESCE((SELECT MAX(created_at) FROM local_unsent WHERE thread_id = threads.id), threads.created_at)) AS active_at
             FROM threads WHERE parent_thread_id = ? ORDER BY created_at, id",
        )
        .bind(parent)
        .fetch_all(&self.pool)
        .await
        .context("list agent threads")?
        .into_iter()
        .map(decode_thread)
        .collect()
    }

    /// The chat a thread belongs to: itself for a chat, the top of its tree for an agent.
    pub async fn root_thread(&self, id: &str) -> Result<String> {
        sqlx::query_scalar(
            "WITH RECURSIVE above(id, parent) AS (
                SELECT id, parent_thread_id FROM threads WHERE id = ?
                UNION ALL
                SELECT threads.id, threads.parent_thread_id
                FROM threads JOIN above ON threads.id = above.parent
             ) SELECT id FROM above WHERE parent IS NULL",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .context("find the chat of a thread")
    }

    /// Every agent thread below this one, at any depth.
    pub async fn descendant_threads(&self, id: &str) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "WITH RECURSIVE below(id) AS (
                SELECT id FROM threads WHERE parent_thread_id = ?
                UNION ALL
                SELECT threads.id FROM threads JOIN below ON threads.parent_thread_id = below.id
             ) SELECT id FROM below",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .context("list agent threads below")
    }

    /// Messages other agents sent a thread since the last one a client sent it.
    pub async fn agent_messages_since_client(&self, id: &str) -> Result<u64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE thread_id = ? AND from_thread_id IS NOT NULL
               AND sequence > (SELECT COALESCE(MAX(sequence), 0) FROM messages
                               WHERE thread_id = ? AND role = 'user' AND from_thread_id IS NULL)",
        )
        .bind(id)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .context("count agent messages")?;
        u64::try_from(count).context("negative message count")
    }

    /// How many agent threads are above this one; 0 for a chat.
    pub async fn thread_depth(&self, id: &str) -> Result<usize> {
        let depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE above(id) AS (
                SELECT parent_thread_id FROM threads WHERE id = ?
                UNION ALL
                SELECT threads.parent_thread_id FROM threads JOIN above ON threads.id = above.id
             ) SELECT COUNT(*) FROM above WHERE id IS NOT NULL",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .context("count thread depth")?;
        usize::try_from(depth).context("negative depth")
    }

    pub async fn set_title(&self, id: &str, title: &str) -> Result<()> {
        ensure!(!title.trim().is_empty(), "title is empty");
        sqlx::query("UPDATE threads SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("set thread title")?;
        Ok(())
    }
}

fn decode_thread(row: SqliteRow) -> Result<Thread> {
    Ok(Thread {
        id: row.try_get("id")?,
        source: row.try_get("source")?,
        created_at: row.try_get("created_at")?,
        title: row.try_get("title")?,
        active_at: row.try_get("active_at")?,
        parent_thread_id: row.try_get("parent_thread_id")?,
        agent: row.try_get("agent")?,
        name: row.try_get("name")?,
        project: row.try_get("project")?,
    })
}

#[cfg(test)]
mod tests {
    use crate::testing::temp_state;

    #[tokio::test]
    async fn a_folder_lists_its_chats_and_those_of_no_known_folder() {
        let state = temp_state("thread-projects").await;
        for (id, dir) in [("here", Some("/a")), ("there", Some("/b")), ("old", None)] {
            state.create_thread(id, "test").await.unwrap();
            if let Some(dir) = dir {
                state.set_thread_project(id, dir).await.unwrap();
            }
        }
        let shown = state.project_threads("/a").await.unwrap();
        let mut ids: Vec<_> = shown.iter().map(|thread| thread.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["here", "old"]);
    }

    #[tokio::test]
    async fn a_stop_and_the_agent_budget_see_the_whole_tree() {
        let state = temp_state("agent_tree").await;
        state.create_thread("chat", "test").await.unwrap();
        state
            .create_agent_thread("a", "chat", "general", "Aster-01")
            .await
            .unwrap();
        state
            .create_agent_thread("b", "a", "general", "Beryl-02")
            .await
            .unwrap();

        assert_eq!(state.root_thread("b").await.unwrap(), "chat");
        assert_eq!(state.root_thread("chat").await.unwrap(), "chat");
        let mut below = state.descendant_threads("chat").await.unwrap();
        below.sort();
        assert_eq!(
            below,
            ["a", "b"],
            "an idle agent does not hide the ones below it"
        );

        for _ in 0..2 {
            let pending = state.add_pending_from("chat", "done", Some("a")).await;
            state
                .admit_pending("chat", &pending.unwrap())
                .await
                .unwrap();
        }
        assert_eq!(state.agent_messages_since_client("chat").await.unwrap(), 2);
        state
            .add_user_message("chat", "user", "go on")
            .await
            .unwrap();
        assert_eq!(state.agent_messages_since_client("chat").await.unwrap(), 0);
    }
}
