//! Threads: one per chat, and one per subagent conversation under the thread that asked
//! it. Everything else in a thread is its messages and turns.

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

pub use dusk_protocol::thread::Thread;

/// The `source` of a thread a team made for a task.
pub const MEMBER_SOURCE: &str = "member";

/// Threads with their folder and when they were last active, then `$rest`. The last
/// message is found by `messages_thread_time`, without reading any message.
macro_rules! select_threads {
    ($rest:literal) => {
        concat!(
            "SELECT threads.*, thread_projects.dir AS project,
                MAX(threads.created_at,
                COALESCE((SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id), threads.created_at),
                COALESCE((SELECT MAX(created_at) FROM local_unsent WHERE thread_id = threads.id), threads.created_at)) AS active_at
             FROM threads LEFT JOIN thread_projects ON thread_projects.thread_id = threads.id ",
            $rest
        )
    };
}

// The walks of the thread tree. UNION, in each: parent links that loop, which only a
// damaged database has, end the walk instead of running it for ever.

/// `$rest` over `family(id)`: the thread bound as `?1` and every agent thread under it.
macro_rules! family {
    ($rest:literal) => {
        concat!(
            "WITH RECURSIVE family(id) AS (SELECT ?1 UNION SELECT threads.id FROM threads ",
            "JOIN family ON threads.parent_thread_id = family.id) ",
            $rest
        )
    };
}
pub(crate) use family;

/// `$rest` over `below(id)`: every agent thread under the thread bound as `?1`, at any
/// depth, without that thread itself.
macro_rules! below {
    ($rest:literal) => {
        concat!(
            "WITH RECURSIVE below(id) AS (SELECT id FROM threads WHERE parent_thread_id = ?1 ",
            "UNION SELECT threads.id FROM threads JOIN below ON threads.parent_thread_id = below.id) ",
            $rest
        )
    };
}
pub(crate) use below;

/// The chat `id` belongs to: itself for a chat, the top of its tree for an agent. `None`
/// for an unknown thread, and for one whose parent links loop.
pub(crate) async fn chat_of<'e>(
    executor: impl sqlx::SqliteExecutor<'e>,
    id: &str,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar(
        "WITH RECURSIVE above(id, parent) AS (
            SELECT id, parent_thread_id FROM threads WHERE id = ?
            UNION
            SELECT threads.id, threads.parent_thread_id
            FROM threads JOIN above ON threads.id = above.parent
         ) SELECT id FROM above WHERE parent IS NULL",
    )
    .bind(id)
    .fetch_optional(executor)
    .await
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

    /// The thread's folder under the workspace root, when it was given one.
    pub async fn thread_folder(&self, id: &str) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT folder FROM thread_folders WHERE thread_id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("read the thread's files folder")
    }

    pub async fn set_thread_folder(&self, id: &str, folder: &str) -> Result<()> {
        sqlx::query("INSERT INTO thread_folders (thread_id, folder) VALUES (?, ?)")
            .bind(id)
            .bind(folder)
            .execute(&self.pool)
            .await
            .context("store the thread's files folder")?;
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
        sqlx::query(select_threads!(
            "WHERE parent_thread_id IS NULL AND source != 'arbiter'
                AND (thread_projects.dir IS NULL OR thread_projects.dir = ?)
             ORDER BY active_at DESC, id DESC"
        ))
        .bind(dir)
        .fetch_all(&self.pool)
        .await
        .context("list the folder's threads")?
        .into_iter()
        .map(decode_thread)
        .collect()
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

    /// A thread the team made under its lead for one task of the plan, working as the
    /// task's role. Its source marks it, so the team finds its own threads again and no
    /// thread an agent asked for is ever taken for one.
    pub async fn create_member_thread(
        &self,
        id: &str,
        lead: &str,
        role: &str,
        name: &str,
    ) -> Result<Thread> {
        self.insert_thread(id, MEMBER_SOURCE, Some(lead), Some(role), Some(name))
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
        sqlx::query(select_threads!("WHERE id = ?"))
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
        sqlx::query(select_threads!(
            "WHERE parent_thread_id IS NULL AND source != 'arbiter'
                 ORDER BY active_at DESC, id DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .context("list threads")?
        .into_iter()
        .map(decode_thread)
        .collect()
    }

    /// Records which token owns a chat, once. Nothing calls it yet: in phase 3 the New,
    /// NewTemporary, Fork and ForkAt handlers will, for the chat they just created. It fails for an unknown thread, a
    /// subagent thread, and a chat another token owns; the same token again is a no-op.
    pub async fn set_thread_owner(&self, thread_id: &str, owner: &str) -> Result<()> {
        ensure!(!owner.trim().is_empty(), "thread owner is empty");
        let changed = sqlx::query(
            "UPDATE threads SET owner = ?
             WHERE id = ? AND parent_thread_id IS NULL AND (owner IS NULL OR owner = ?)",
        )
        .bind(owner)
        .bind(thread_id)
        .bind(owner)
        .execute(&self.pool)
        .await
        .context("store the thread's owner")?
        .rows_affected();
        ensure!(changed == 1, "thread cannot take this owner");
        Ok(())
    }

    /// The id of the token that owns the chat, `None` when no token does.
    pub async fn thread_owner(&self, thread_id: &str) -> Result<Option<String>> {
        let owner: Option<Option<String>> =
            sqlx::query_scalar("SELECT owner FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the thread's owner")?;
        Ok(owner.flatten())
    }

    /// The chats a token owns, oldest first. Subagent threads are not listed.
    pub async fn threads_owned_by(&self, owner: &str) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT id FROM threads WHERE owner = ? AND parent_thread_id IS NULL
             ORDER BY created_at, id",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .context("list the threads a token owns")
    }

    /// The subagent threads a thread started, oldest first.
    pub async fn agent_threads(&self, parent: &str) -> Result<Vec<Thread>> {
        sqlx::query(select_threads!(
            "WHERE parent_thread_id = ? ORDER BY created_at, id"
        ))
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
        chat_of(&self.pool, id)
            .await
            .context("find the chat of a thread")?
            .context("the thread has no chat")
    }

    /// Every agent thread below this one, at any depth.
    pub async fn descendant_threads(&self, id: &str) -> Result<Vec<String>> {
        sqlx::query_scalar(below!("SELECT id FROM below"))
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
                UNION
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
        sqlx::query("UPDATE threads SET title = ?, auto_title = 0 WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("set thread title")?;
        Ok(())
    }

    /// A first message gets a stand-in only if the thread does not have a title yet.
    pub async fn set_first_title(&self, id: &str, title: &str, pending: bool) -> Result<bool> {
        ensure!(!title.trim().is_empty(), "title is empty");
        let rows = sqlx::query(
            "UPDATE threads SET title = ?, auto_title = ? WHERE id = ? AND title IS NULL",
        )
        .bind(title)
        .bind(pending)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("set the first thread title")?
        .rows_affected();
        Ok(rows != 0)
    }

    /// Claims the only automatic title request, even if that request later fails.
    pub async fn claim_pending_title(&self, id: &str) -> Result<bool> {
        let rows = sqlx::query("UPDATE threads SET auto_title = 2 WHERE id = ? AND auto_title = 1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("claim the thread title attempt")?
            .rows_affected();
        Ok(rows != 0)
    }

    /// A title chosen while the model was answering takes precedence over its answer.
    pub async fn finish_title_attempt(&self, id: &str, title: &str) -> Result<bool> {
        ensure!(!title.trim().is_empty(), "title is empty");
        let rows = sqlx::query(
            "UPDATE threads SET title = ?, auto_title = 0 WHERE id = ? AND auto_title = 2",
        )
        .bind(title)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("finish the thread title")?
        .rows_affected();
        Ok(rows != 0)
    }
}

impl State {
    /// Erases a chat and the threads of every subagent under it, with all their records,
    /// for good. Answers with their files folders, for the caller to remove. Records with
    /// no foreign key to a thread stay: the skill changes, kept for undo, and a schedule's
    /// last thread.
    pub async fn delete_thread(&self, id: &str) -> Result<Vec<String>> {
        let mut transaction = self.pool.begin().await.context("begin the delete")?;
        // Every check waits for the commit, when no row may point at a deleted one.
        sqlx::query("PRAGMA defer_foreign_keys = ON")
            .execute(&mut *transaction)
            .await?;
        let ids: Vec<String> = sqlx::query_scalar(family!("SELECT id FROM family"))
            .bind(id)
            .fetch_all(&mut *transaction)
            .await
            .context("find the chat's threads")?;
        let known: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await?;
        ensure!(known == 1 && !ids.is_empty(), "no such chat");
        let folders: Vec<String> = sqlx::query_scalar(family!(
            "SELECT folder FROM thread_folders WHERE thread_id IN (SELECT id FROM family)"
        ))
        .bind(id)
        .fetch_all(&mut *transaction)
        .await?;
        // The keys that name a row, read from the schema so a table added later is covered
        // too. A key with its own ON DELETE action is left to it.
        let keys: Vec<(String, String, String, String, bool)> = sqlx::query_as(
            "SELECT m.name, f.\"from\", f.\"table\", f.\"to\", c.\"notnull\"
             FROM sqlite_master m, pragma_foreign_key_list(m.name) f, pragma_table_info(m.name) c
             WHERE m.type = 'table' AND m.name != 'threads' AND f.on_delete = 'NO ACTION'
                AND c.name = f.\"from\"",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let family = serde_json::to_string(&ids).context("list the chat's threads")?;
        // Names come from the schema itself, never from a caller. A row is the chat's when
        // any of its thread columns names one of the chat's threads.
        // As a search, each column found by its index; as a filter, never NULL.
        let of_chat = |table: &str, filter: bool| {
            let columns: Vec<String> = keys
                .iter()
                .filter(|(child, _, parent, _, _)| child == table && parent == "threads")
                .map(|(_, column, ..)| {
                    let named =
                        format!("\"{table}\".\"{column}\" IN (SELECT value FROM json_each(?1))");
                    if filter {
                        format!("COALESCE({named}, 0)")
                    } else {
                        named
                    }
                })
                .collect();
            (!columns.is_empty()).then(|| format!("({})", columns.join(" OR ")))
        };
        // First the rows of other chats, and the rows of no thread, that point at a row of
        // the chat by another key: a link that may be empty is cleared, so another chat
        // keeps its record; any other such row goes. No chat is left undeletable by a row
        // that points into it.
        for (table, column, parent, key, required) in &keys {
            let Some(doomed) = of_chat(parent, false) else {
                continue;
            };
            let points = format!(
                "\"{table}\".\"{column}\" IN (SELECT \"{key}\" FROM \"{parent}\" WHERE {doomed})"
            );
            let sql = match of_chat(table, true) {
                // The chat's own rows go below.
                Some(own) if !required => format!(
                    "UPDATE \"{table}\" SET \"{column}\" = NULL WHERE {points} AND NOT {own}"
                ),
                Some(own) => format!("DELETE FROM \"{table}\" WHERE {points} AND NOT {own}"),
                None => format!("DELETE FROM \"{table}\" WHERE {points}"),
            };
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&family)
                .execute(&mut *transaction)
                .await
                .with_context(|| format!("unlink {table} from the chat"))?;
        }
        // Then every row of the chat's threads, found by the column that names the thread.
        for (table, column, parent, ..) in &keys {
            if parent != "threads" {
                continue;
            }
            let sql = format!(
                "DELETE FROM \"{table}\" WHERE \"{column}\" IN (SELECT value FROM json_each(?1))"
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&family)
                .execute(&mut *transaction)
                .await
                .with_context(|| format!("delete the chat's rows of {table}"))?;
        }
        sqlx::query("DELETE FROM threads WHERE id IN (SELECT value FROM json_each(?))")
            .bind(&family)
            .execute(&mut *transaction)
            .await
            .context("delete the chat")?;
        // Foreign keys are checked here, so a key the steps above missed fails the delete
        // instead of leaving a row dangling.
        transaction.commit().await.context("commit the delete")?;
        Ok(folders)
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
        // The runtime marks the threads of its database in memory.
        temporary: false,
        owner: row.try_get("owner")?,
        unread: row.try_get("unread")?,
    })
}

#[cfg(test)]
mod tests {
    use crate::state::State;
    use crate::testing::{temp_dir, temp_state};
    use std::time::Duration;

    #[tokio::test]
    async fn parent_links_that_loop_end_the_walk_up_and_down() {
        let state = temp_state("thread_loop").await;
        state.create_thread("chat", "test").await.unwrap();
        for (id, parent) in [("one", "chat"), ("two", "one")] {
            state
                .create_agent_thread(id, parent, "agent", id)
                .await
                .unwrap();
        }
        assert_eq!(state.root_thread("two").await.unwrap(), "chat");
        assert_eq!(state.thread_depth("two").await.unwrap(), 2);
        // No code writes this; a damaged file could hold it.
        sqlx::query("UPDATE threads SET parent_thread_id = 'two' WHERE id = 'one'")
            .execute(&state.pool)
            .await
            .unwrap();

        let soon = Duration::from_secs(5);
        let root = tokio::time::timeout(soon, state.root_thread("two")).await;
        assert!(root.expect("the walk up never ends").is_err());
        let below = tokio::time::timeout(soon, state.descendant_threads("one")).await;
        let mut below = below.expect("the walk down never ends").unwrap();
        below.sort();
        assert_eq!(below, ["one", "two"]);
        let depth = tokio::time::timeout(soon, state.thread_depth("two")).await;
        assert_eq!(depth.expect("the count never ends").unwrap(), 2);
        // Every other walk of the tree shares the same SQL, and ends too.
        let tokens = tokio::time::timeout(soon, state.thread_tokens("one")).await;
        tokens.expect("the token sum never ends").unwrap();
        let wake = tokio::time::timeout(soon, state.goal_wake_state("one")).await;
        assert_eq!(
            wake.expect("the wake check never ends").unwrap(),
            (false, false)
        );
        let busy = sqlx::query_scalar::<_, bool>(below!(
            "SELECT EXISTS(SELECT 1 FROM below WHERE id = 'two')"
        ))
        .bind("one")
        .fetch_one(&state.pool);
        assert!(tokio::time::timeout(soon, busy).await.unwrap().unwrap());
        let family = sqlx::query_scalar::<_, String>(family!("SELECT id FROM family ORDER BY id"))
            .bind("one")
            .fetch_all(&state.pool);
        let family = tokio::time::timeout(soon, family).await.unwrap().unwrap();
        assert_eq!(family, ["one", "two"]);
    }

    #[tokio::test]
    async fn the_thread_list_finds_the_last_message_by_index() {
        let state = temp_state("thread_list_plan").await;
        let sql = concat!("EXPLAIN QUERY PLAN ", select_threads!("WHERE id = ?"));
        let plan = crate::testing::query_plan(&state, sql).await;
        assert!(plan.contains("messages_thread_time"), "{plan}");
    }

    #[tokio::test]
    async fn a_manual_title_takes_precedence_over_an_in_flight_generated_title() {
        let state = crate::testing::temp_state("manual_title_during_generation").await;
        state.create_thread("thread", "test").await.unwrap();
        assert!(
            state
                .set_first_title("thread", "first question", true)
                .await
                .unwrap()
        );
        assert!(state.claim_pending_title("thread").await.unwrap());
        assert!(!state.claim_pending_title("thread").await.unwrap());

        // The user can even choose the same text as the stand-in.
        state.set_title("thread", "first question").await.unwrap();
        assert!(
            !state
                .finish_title_attempt("thread", "generated title")
                .await
                .unwrap()
        );
        let thread = state.load_thread("thread").await.unwrap().unwrap();
        assert_eq!(thread.title.as_deref(), Some("first question"));
    }

    #[tokio::test]
    async fn a_deleted_chat_takes_its_agents_and_records() {
        let state = temp_state("thread-delete").await;
        state.create_thread("chat", "gui").await.unwrap();
        state
            .create_agent_thread("agent", "chat", "general", "Fable-1")
            .await
            .unwrap();
        state.create_thread("other", "gui").await.unwrap();
        state
            .set_thread_folder("chat", "2026-11-05-chat")
            .await
            .unwrap();
        for id in ["chat", "agent", "other"] {
            state
                .add_user_message(id, &format!("r-{id}"), "hello")
                .await
                .unwrap();
            state
                .add_task(id, "work", &["one".to_owned()])
                .await
                .unwrap();
        }
        // A turn with a tool call and its result, and a report the agent sent the chat.
        let pending = state.add_pending("chat", "go").await.unwrap();
        state.admit_pending("chat", &pending).await.unwrap();
        let turn = state.start_turn("chat").await.unwrap().unwrap().turn_id;
        let call = crate::providers::ToolCall {
            id: "c".into(),
            name: "echo".into(),
            arguments: serde_json::json!({}),
        };
        let answer = crate::state::TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        state
            .record_tool_calls(&turn, answer, vec![call])
            .await
            .unwrap();
        state
            .record_tool_result_with_artifacts(&turn, "c", "shown".into(), Vec::new())
            .await
            .unwrap();
        let report = state.add_pending_from("chat", "done", Some("agent")).await;
        state.admit_pending("chat", &report.unwrap()).await.unwrap();

        let folders = state.delete_thread("chat").await.unwrap();
        assert_eq!(folders, ["2026-11-05-chat"]);
        // Nothing is left pointing at a deleted row, and the search index has no trace.
        let dangling = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&state.pool)
            .await
            .unwrap();
        assert!(dangling.is_empty());
        sqlx::query("INSERT INTO messages_search (messages_search) VALUES ('integrity-check')")
            .execute(&state.pool)
            .await
            .unwrap();
        let found = state.search_messages("other", "hello", 10).await.unwrap();
        assert_eq!(found.len(), 1);
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages_search WHERE messages_search MATCH 'shown'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(found, 0);
        let left: Vec<String> = state
            .list_threads()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(left, ["other"]);
        assert!(state.list_messages("chat").await.unwrap().is_empty());
        assert_eq!(state.list_messages("other").await.unwrap().len(), 1);
        assert_eq!(state.thread_tasks("other").await.unwrap().len(), 1);
        assert!(state.delete_thread("chat").await.is_err());
    }

    #[tokio::test]
    async fn a_row_of_another_chat_that_points_into_a_chat_does_not_keep_it() {
        let state = temp_state("thread-delete-cross").await;
        state.create_thread("chat", "gui").await.unwrap();
        state.create_thread("other", "gui").await.unwrap();
        let pending = state.add_pending("chat", "go").await.unwrap();
        state.admit_pending("chat", &pending).await.unwrap();
        let turn = state.start_turn("chat").await.unwrap().unwrap().turn_id;
        let task = state.add_task("chat", "work", &[]).await.unwrap().task_id;
        let pending = state.add_pending("other", "go").await.unwrap();
        state.admit_pending("other", &pending).await.unwrap();
        let theirs = state.start_turn("other").await.unwrap().unwrap().turn_id;
        // No code writes these today; a later feature could. Usage of the other chat
        // charged to this chat's turn, and a turn of it that worked on this chat's task.
        sqlx::query(
            "INSERT INTO model_usage (thread_id, turn_id, purpose, model, through_sequence,
                prompt_tokens, cached_prompt_tokens, completion_tokens, reasoning_tokens,
                created_at) VALUES ('other', ?, 'answer', 'm', 1, 1, 0, 1, 0, 1)",
        )
        .bind(&turn)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE turns SET task_id = ? WHERE turn_id = ?")
            .bind(&task)
            .bind(&theirs)
            .execute(&state.pool)
            .await
            .unwrap();

        state.delete_thread("chat").await.unwrap();
        let dangling = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&state.pool)
            .await
            .unwrap();
        assert!(dangling.is_empty());
        // The other chat keeps its records, without the links.
        let usage: Vec<Option<String>> =
            sqlx::query_scalar("SELECT turn_id FROM model_usage WHERE thread_id = 'other'")
                .fetch_all(&state.pool)
                .await
                .unwrap();
        assert_eq!(usage, [None]);
        let linked: Option<String> =
            sqlx::query_scalar("SELECT task_id FROM turns WHERE turn_id = ?")
                .bind(&theirs)
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(linked, None);
    }

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

    #[tokio::test]
    async fn an_old_database_gets_owner_null() {
        let path = temp_dir("owner_migration").join("state.db");
        let older: Vec<_> = sqlx::migrate!("src/state/db/migrations")
            .iter()
            .filter(|migration| migration.version < 41)
            .cloned()
            .collect();
        assert_eq!(older.last().map(|migration| migration.version), Some(40));
        let migrator = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(older),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        migrator.run(&pool).await.unwrap();
        sqlx::raw_sql("INSERT INTO threads (id, source, created_at) VALUES ('old', 'test', 1)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let state = State::open(&path).await.unwrap();
        assert_eq!(state.thread_owner("old").await.unwrap(), None);
        let threads = state.list_threads().await.unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].owner, None);
        state.close().await;
    }

    #[tokio::test]
    async fn owner_is_set_once_and_listed() {
        let state = temp_state("thread_owner").await;
        state.create_thread("a", "test").await.unwrap();
        state.create_thread("b", "test").await.unwrap();
        state
            .create_agent_thread("agent", "a", "general", "agent")
            .await
            .unwrap();
        state.set_thread_owner("a", "t1").await.unwrap();
        assert!(state.set_thread_owner("agent", "t1").await.is_err());
        state.set_thread_owner("a", "t1").await.unwrap();
        assert!(state.set_thread_owner("a", "t2").await.is_err());
        assert_eq!(
            state.thread_owner("a").await.unwrap().as_deref(),
            Some("t1")
        );
        assert_eq!(state.thread_owner("agent").await.unwrap(), None);
        assert_eq!(state.thread_owner("b").await.unwrap(), None);
        assert_eq!(state.threads_owned_by("t1").await.unwrap(), ["a"]);
        assert!(state.threads_owned_by("t2").await.unwrap().is_empty());
        assert!(state.set_thread_owner("ghost", "t1").await.is_err());
        assert!(state.set_thread_owner("b", " ").await.is_err());
        let listed = state.load_thread("a").await.unwrap().unwrap();
        assert_eq!(listed.owner.as_deref(), Some("t1"));
    }
}
