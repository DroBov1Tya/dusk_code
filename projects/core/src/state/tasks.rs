//! The tasks an agent sets itself in a thread, and their steps. Every query is bound to
//! the thread, so an agent only ever sees and changes its own.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub task_id: String,
    pub description: String,
    pub done: bool,
    /// In order.
    pub items: Vec<TaskItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub item_id: String,
    pub description: String,
    pub done: bool,
}

/// What an ID the agent gave turned out to be.
enum Found {
    Task(String),
    Item(String),
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
        let mut transaction = self.pool.begin().await.context("begin task")?;
        sqlx::query(
            "INSERT INTO tasks (task_id, thread_id, description, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&task_id)
        .bind(thread_id)
        .bind(description.trim())
        .bind(timestamp_ms()?)
        .execute(&mut *transaction)
        .await
        .context("store task")?;
        transaction.commit().await.context("commit task")?;
        self.add_task_items(thread_id, &task_id, items).await
    }

    /// Appends steps to a task of the thread; `task` may be the end of its ID.
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
        for item in items.iter().filter(|item| !item.trim().is_empty()) {
            sqlx::query(
                "INSERT INTO task_items (item_id, task_id, position, description)
                 VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM task_items
                                WHERE task_id = ?), ?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&task_id)
            .bind(&task_id)
            .bind(item.trim())
            .execute(&mut *transaction)
            .await
            .context("store task step")?;
        }
        sqlx::query("UPDATE tasks SET done = 0 WHERE task_id = ?")
            .bind(&task_id)
            .execute(&mut *transaction)
            .await
            .context("reopen task")?;
        transaction.commit().await.context("commit task steps")?;
        self.task(&task_id).await
    }

    /// Marks a step done, or a whole task with every step of it. `id` may be its end.
    pub async fn complete(&self, thread_id: &str, id: &str) -> Result<Task> {
        let task_id = match self.find(thread_id, id).await? {
            Found::Task(task_id) => {
                sqlx::query("UPDATE task_items SET done = 1 WHERE task_id = ?")
                    .bind(&task_id)
                    .execute(&self.pool)
                    .await
                    .context("complete task steps")?;
                sqlx::query("UPDATE tasks SET done = 1 WHERE task_id = ?")
                    .bind(&task_id)
                    .execute(&self.pool)
                    .await
                    .context("complete task")?;
                task_id
            }
            Found::Item(item_id) => {
                let task_id: String = sqlx::query_scalar(
                    "UPDATE task_items SET done = 1 WHERE item_id = ? RETURNING task_id",
                )
                .bind(&item_id)
                .fetch_one(&self.pool)
                .await
                .context("complete task step")?;
                task_id
            }
        };
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
        let row = sqlx::query("SELECT task_id, description, done FROM tasks WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&self.pool)
            .await
            .context("load task")?;
        let items = sqlx::query(
            "SELECT item_id, description, done FROM task_items WHERE task_id = ? ORDER BY position",
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
            })
        })
        .collect::<Result<_>>()?;
        Ok(Task {
            task_id: row.try_get("task_id")?,
            description: row.try_get("description")?,
            done: is_done(&row)?,
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
        let items: Vec<String> = sqlx::query_scalar(
            "SELECT task_items.item_id FROM task_items JOIN tasks USING (task_id)
             WHERE tasks.thread_id = ? AND task_items.item_id LIKE ?",
        )
        .bind(thread_id)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .context("find task step")?;
        match (tasks.as_slice(), items.as_slice()) {
            ([task], []) => Ok(Found::Task(task.clone())),
            ([], [item]) => Ok(Found::Item(item.clone())),
            ([], []) => bail!("no task or step {id} in this thread"),
            _ => bail!("{id} fits more than one; give more of it"),
        }
    }
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
}
