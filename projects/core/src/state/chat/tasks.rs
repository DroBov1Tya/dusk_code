//! The tasks an agent sets itself in a thread, and their steps. Every query is bound to
//! the thread, so an agent only ever sees and changes its own.
//!
//! A task holds the IDs of its steps in order. Steps run one after another: when one is
//! done the next starts, so each has a timer without the agent keeping one. A task is
//! done once all of its steps are, or when the agent says so.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub task_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// In order.
    pub items: Vec<TaskItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub item_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// `None` until work on it begins.
    pub started_at: Option<i64>,
    /// Set exactly when it is done.
    pub finished_at: Option<i64>,
}

/// What an ID the agent gave turned out to be.
enum Found {
    Task(String),
    Item { task_id: String, item_id: String },
}

impl State {
    pub async fn add_task(
        &self,
        thread_id: &str,
        description: &str,
        items: &[String],
    ) -> Result<Task> {
        ensure!(
            !description.trim().is_empty(),
            "the task has no description"
        );
        let task_id = Uuid::now_v7().to_string();
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin task")?;
        sqlx::query(
            "INSERT INTO tasks (task_id, thread_id, description, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&task_id)
        .bind(thread_id)
        .bind(description.trim())
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .context("store task")?;
        append_items(&mut transaction, &task_id, items, now).await?;
        transaction.commit().await.context("commit task")?;
        self.task(&task_id).await
    }

    /// Appends steps to a task of the thread, which opens it again; `task` may be the end
    /// of its ID.
    pub async fn add_task_items(
        &self,
        thread_id: &str,
        task: &str,
        items: &[String],
    ) -> Result<Task> {
        let Found::Task(task_id) = self.find(thread_id, task).await? else {
            bail!("{task} is a step, not a task");
        };
        let mut transaction = self.pool.begin().await.context("begin task steps")?;
        append_items(&mut transaction, &task_id, items, timestamp_ms()?).await?;
        transaction.commit().await.context("commit task steps")?;
        self.task(&task_id).await
    }

    /// Starts the timer of a step out of order, as when steps run side by side.
    pub async fn start_item(&self, thread_id: &str, id: &str) -> Result<Task> {
        let Found::Item { task_id, item_id } = self.find(thread_id, id).await? else {
            bail!("{id} is a task; start one of its steps");
        };
        let now = timestamp_ms()?;
        sqlx::query(
            "UPDATE task_items SET started_at = ?, updated_at = ?
             WHERE item_id = ? AND started_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(&item_id)
        .execute(&self.pool)
        .await
        .context("start task step")?;
        self.task(&task_id).await
    }

    /// Marks a step done and starts the next one, or a whole task with every step of it.
    /// `id` may be its end.
    pub async fn complete(&self, thread_id: &str, id: &str) -> Result<Task> {
        let found = self.find(thread_id, id).await?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin completion")?;
        let task_id = match found {
            Found::Task(task_id) => {
                sqlx::query(
                    "UPDATE task_items SET done = 1, started_at = COALESCE(started_at, ?1),
                         finished_at = ?1, updated_at = ?1
                     WHERE task_id = ?2 AND done = 0",
                )
                .bind(now)
                .bind(&task_id)
                .execute(&mut *transaction)
                .await
                .context("complete task steps")?;
                sqlx::query("UPDATE tasks SET done = 1, updated_at = ? WHERE task_id = ?")
                    .bind(now)
                    .bind(&task_id)
                    .execute(&mut *transaction)
                    .await
                    .context("complete task")?;
                task_id
            }
            Found::Item { task_id, item_id } => {
                sqlx::query(
                    "UPDATE task_items SET done = 1, started_at = COALESCE(started_at, ?1),
                         finished_at = ?1, updated_at = ?1
                     WHERE item_id = ?2 AND done = 0",
                )
                .bind(now)
                .bind(&item_id)
                .execute(&mut *transaction)
                .await
                .context("complete task step")?;
                settle(&mut transaction, &task_id, now).await?;
                task_id
            }
        };
        transaction.commit().await.context("commit completion")?;
        self.task(&task_id).await
    }

    /// Rewrites what a task or a step says.
    pub async fn edit_task(&self, thread_id: &str, id: &str, description: &str) -> Result<Task> {
        ensure!(!description.trim().is_empty(), "the new text is empty");
        let found = self.find(thread_id, id).await?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin edit")?;
        let task_id = match found {
            Found::Task(task_id) => {
                sqlx::query("UPDATE tasks SET description = ? WHERE task_id = ?")
                    .bind(description.trim())
                    .bind(&task_id)
                    .execute(&mut *transaction)
                    .await
                    .context("edit task")?;
                task_id
            }
            Found::Item { task_id, item_id } => {
                sqlx::query(
                    "UPDATE task_items SET description = ?, updated_at = ? WHERE item_id = ?",
                )
                .bind(description.trim())
                .bind(now)
                .bind(&item_id)
                .execute(&mut *transaction)
                .await
                .context("edit task step")?;
                task_id
            }
        };
        sqlx::query("UPDATE tasks SET updated_at = ? WHERE task_id = ?")
            .bind(now)
            .bind(&task_id)
            .execute(&mut *transaction)
            .await
            .context("touch task")?;
        transaction.commit().await.context("commit edit")?;
        self.task(&task_id).await
    }

    /// Every task of the thread with its steps, oldest first.
    pub async fn thread_tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM tasks WHERE thread_id = ? ORDER BY created_at, task_id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("list tasks")?;
        let mut tasks = Vec::with_capacity(ids.len());
        for task_id in ids {
            tasks.push(self.task(&task_id).await?);
        }
        Ok(tasks)
    }

    async fn task(&self, task_id: &str) -> Result<Task> {
        let row = sqlx::query(
            "SELECT task_id, description, done, created_at, updated_at FROM tasks
             WHERE task_id = ?",
        )
        .bind(task_id)
        .fetch_one(&self.pool)
        .await
        .context("load task")?;
        let items = sqlx::query(
            "SELECT i.item_id, i.description, i.done, i.created_at, i.updated_at,
                    i.started_at, i.finished_at
             FROM tasks t, json_each(t.item_ids) j JOIN task_items i ON i.item_id = j.value
             WHERE t.task_id = ? ORDER BY j.key",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .context("load task steps")?
        .into_iter()
        .map(|row| {
            Ok(TaskItem {
                item_id: row.try_get("item_id")?,
                description: row.try_get("description")?,
                done: is_done(&row)?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
                started_at: row.try_get("started_at")?,
                finished_at: row.try_get("finished_at")?,
            })
        })
        .collect::<Result<_>>()?;
        Ok(Task {
            task_id: row.try_get("task_id")?,
            description: row.try_get("description")?,
            done: is_done(&row)?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            items,
        })
    }

    /// A task or step of the thread by its ID or the end of it, when only one fits. The
    /// end, because a UUIDv7 starts with its time, which steps made together share.
    async fn find(&self, thread_id: &str, id: &str) -> Result<Found> {
        let id = id.trim();
        ensure!(id.len() >= 4, "give at least four characters of the ID");
        let pattern = format!("%{}", id.replace(['%', '_'], ""));
        let tasks: Vec<String> =
            sqlx::query_scalar("SELECT task_id FROM tasks WHERE thread_id = ? AND task_id LIKE ?")
                .bind(thread_id)
                .bind(&pattern)
                .fetch_all(&self.pool)
                .await
                .context("find task")?;
        let items: Vec<(String, String)> = sqlx::query_as(
            "SELECT task_items.task_id, task_items.item_id
             FROM task_items JOIN tasks USING (task_id)
             WHERE tasks.thread_id = ? AND task_items.item_id LIKE ?",
        )
        .bind(thread_id)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .context("find task step")?;
        match (tasks.as_slice(), items.as_slice()) {
            ([task], []) => Ok(Found::Task(task.clone())),
            ([], [(task_id, item_id)]) => Ok(Found::Item {
                task_id: task_id.clone(),
                item_id: item_id.clone(),
            }),
            ([], []) => bail!("no task or step {id} in this thread"),
            _ => bail!("{id} fits more than one; give more of it"),
        }
    }
}

/// Stores new steps at the end of the task's list.
async fn append_items(
    transaction: &mut Transaction<'_, Sqlite>,
    task_id: &str,
    items: &[String],
    now: i64,
) -> Result<()> {
    for item in items.iter().filter(|item| !item.trim().is_empty()) {
        let item_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO task_items (item_id, task_id, description, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&item_id)
        .bind(task_id)
        .bind(item.trim())
        .bind(now)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .context("store task step")?;
        sqlx::query(
            "UPDATE tasks SET item_ids = json_insert(item_ids, '$[#]', ?) WHERE task_id = ?",
        )
        .bind(&item_id)
        .bind(task_id)
        .execute(&mut **transaction)
        .await
        .context("list task step")?;
    }
    settle(transaction, task_id, now).await
}

/// After the steps changed: the first open step starts when none runs, and the task is
/// done exactly when it has steps and all of them are.
async fn settle(transaction: &mut Transaction<'_, Sqlite>, task_id: &str, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE task_items SET started_at = ?1, updated_at = ?1
         WHERE item_id = (
             SELECT i.item_id FROM tasks t, json_each(t.item_ids) j
             JOIN task_items i ON i.item_id = j.value
             WHERE t.task_id = ?2 AND i.done = 0 ORDER BY j.key LIMIT 1)
           AND started_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM task_items
                           WHERE task_id = ?2 AND done = 0 AND started_at IS NOT NULL)",
    )
    .bind(now)
    .bind(task_id)
    .execute(&mut **transaction)
    .await
    .context("start the next task step")?;
    sqlx::query(
        "UPDATE tasks SET updated_at = ?1,
             done = EXISTS (SELECT 1 FROM task_items WHERE task_id = ?2)
                AND NOT EXISTS (SELECT 1 FROM task_items WHERE task_id = ?2 AND done = 0)
         WHERE task_id = ?2",
    )
    .bind(now)
    .bind(task_id)
    .execute(&mut **transaction)
    .await
    .context("settle task")?;
    Ok(())
}

fn is_done(row: &SqliteRow) -> Result<bool> {
    let done: i64 = row.try_get("done")?;
    Ok(done == 1)
}

#[cfg(test)]
mod tests {
    use crate::testing::temp_state;

    #[tokio::test]
    async fn an_agent_reaches_only_its_own_tasks() {
        let state = temp_state("tasks_own").await;
        state.create_thread("mine", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        let steps = vec!["read".to_owned(), "write".to_owned()];
        let task = state.add_task("mine", "port it", &steps).await.unwrap();
        let first = task.items[0].item_id.clone();

        assert!(
            state.complete("other", &first).await.is_err(),
            "another thread's step"
        );
        assert!(
            state.complete("other", &task.task_id).await.is_err(),
            "another thread's task"
        );

        let task = state
            .complete("mine", &first[first.len() - 8..])
            .await
            .unwrap();
        assert!(task.items[0].done && !task.items[1].done && !task.done);
        let task = state.complete("mine", &task.task_id).await.unwrap();
        assert!(task.done && task.items.iter().all(|item| item.done));
    }

    #[tokio::test]
    async fn steps_run_one_after_another() {
        let state = temp_state("tasks_timers").await;
        state.create_thread("mine", "test").await.unwrap();
        let steps = vec!["read".to_owned(), "write".to_owned()];
        let task = state.add_task("mine", "port it", &steps).await.unwrap();
        let (read, write) = (&task.items[0], &task.items[1]);
        assert!(read.started_at.is_some() && write.started_at.is_none());

        let task = state.complete("mine", &read.item_id).await.unwrap();
        let (read, write) = (&task.items[0], &task.items[1]);
        assert!(read.finished_at.is_some() && write.started_at.is_some());

        let task = state.complete("mine", &write.item_id).await.unwrap();
        assert!(task.done, "the last step closes the task");
        let more = vec!["test".to_owned()];
        let task = state
            .add_task_items("mine", &task.task_id, &more)
            .await
            .unwrap();
        assert!(!task.done && task.items[2].started_at.is_some());
        assert_eq!(task.items[2].description, "test");
    }
}
