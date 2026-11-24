//! The tasks an agent sets itself in a thread, and their steps. Every query of this file
//! is bound to the thread, so an agent only ever sees and changes its own.
//!
//! A task holds the IDs of its steps in order. Steps run one after another: when one is
//! done the next starts, so each has a timer without the agent keeping one. A task is
//! done once all of its steps are, or when the agent says so.
//!
//! The same table is a chat's plan: the tasks of its root thread, shared by the threads
//! under it, each with a status, dependencies and an owner. `plan` has its words,
//! `graph` writes and reads it, `claims` moves a task from one status to the next. The
//! board here knows two of the statuses, open and done, and leaves a task in any other
//! to the plan.

mod barrier;
mod claims;
mod duties;
mod error;
mod files;
mod graph;
mod plan;
mod view;

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};

pub use barrier::{current_wave, waves};
pub use claims::{Claim, PassedBy};
pub use dusk_protocol::thread::Task;
pub use duties::Duty;
pub use error::{PlanError, Unclaimable, short_task_id};
pub use files::{TaskFiles, overlaps};
pub(super) use graph::root_of;
pub use plan::{
    BlockedOn, LEASE_MS, MAX_ATTEMPTS, MAX_DEPENDENCIES, MAX_FILES, MAX_GOAL, MAX_NOTE,
    MAX_OPEN_TASKS, MAX_RESULT, MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PlanStatus, PlanTask,
    TaskEdit, WaitsOn,
};
use plan::{decode, select_tasks};

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
        // A model that lost track of its board adds the same task again; one is enough.
        let open: Vec<(String, String)> = sqlx::query_as(
            "SELECT task_id, description FROM tasks WHERE thread_id = ? AND done = 0",
        )
        .bind(thread_id)
        .fetch_all(&mut *transaction)
        .await
        .context("read open tasks")?;
        let words = description.trim().to_lowercase();
        if let Some((open_id, _)) = open
            .iter()
            .find(|(_, open)| open.trim().to_lowercase() == words)
        {
            let short = &open_id[open_id.len().saturating_sub(8)..];
            bail!(
                "task #{short} with these words is already open, nothing was added; \
                 add steps to it with add_items or look at the board with show"
            );
        }
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
        // New steps open a done task again. One that a member handed in, or that another
        // task waited for, stays done: what was built on it would lose its ground.
        let (owned, needed): (bool, bool) = sqlx::query_as(
            "SELECT t.owner_thread_id IS NOT NULL,
                    EXISTS (SELECT 1 FROM tasks o, json_each(o.depends_on) d
                            WHERE o.thread_id = t.thread_id AND d.value = t.task_id)
             FROM tasks t WHERE t.task_id = ? AND t.status = 'done'",
        )
        .bind(&task_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("read the task")?
        .unwrap_or_default();
        ensure!(
            !owned && !needed,
            "{task} is done and other work stands on it; add a new task instead"
        );
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
                // A task the team has given out is done by its review, not from here.
                let closed = sqlx::query(
                    "UPDATE tasks SET done = 1, status = 'done', updated_at = ?
                     WHERE task_id = ? AND status IN ('open', 'done')",
                )
                .bind(now)
                .bind(&task_id)
                .execute(&mut *transaction)
                .await
                .context("complete task")?
                .rows_affected();
                ensure!(
                    closed == 1,
                    "{id} is with the team; its review decides when it is done"
                );
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
        let rows = sqlx::query(select_tasks!(
            "WHERE t.thread_id = ? ORDER BY t.created_at, t.task_id"
        ))
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("list tasks")?;
        let tasks = rows.iter().map(decode).collect::<Result<Vec<_>, _>>()?;
        let mut connection = self.pool.acquire().await.context("list tasks")?;
        Ok(view::present(&mut connection, tasks).await?)
    }

    async fn task(&self, task_id: &str) -> Result<Task> {
        let row = sqlx::query(select_tasks!("WHERE t.task_id = ?"))
            .bind(task_id)
            .fetch_one(&self.pool)
            .await
            .context("load task")?;
        let mut connection = self.pool.acquire().await.context("load task")?;
        let mut shown = view::present(&mut connection, vec![decode(&row)?]).await?;
        shown.pop().context("load task")
    }

    /// A task or step of the thread by its ID or the end of it, when only one fits. The
    /// end, because a UUIDv7 starts with its time, which steps made together share.
    async fn find(&self, thread_id: &str, id: &str) -> Result<Found> {
        let given = id.trim().trim_start_matches('#');
        ensure!(
            given.chars().count() >= 4,
            "give at least four characters of the ID"
        );
        let Some((id, length)) = graph::id_end(given) else {
            bail!("no task or step {given} in this thread");
        };
        let tasks: Vec<String> = sqlx::query_scalar(concat!(
            "SELECT task_id FROM tasks WHERE thread_id = ?1 AND ",
            graph::fits_id!("task_id")
        ))
        .bind(thread_id)
        .bind(&id)
        .bind(length)
        .fetch_all(&self.pool)
        .await
        .context("find task")?;
        let items: Vec<(String, String)> = sqlx::query_as(concat!(
            "SELECT task_items.task_id, task_items.item_id
             FROM task_items JOIN tasks USING (task_id)
             WHERE tasks.thread_id = ?1 AND ",
            graph::fits_id!("task_items.item_id")
        ))
        .bind(thread_id)
        .bind(&id)
        .bind(length)
        .fetch_all(&self.pool)
        .await
        .context("find task step")?;
        let found = tasks.into_iter().map(Found::Task).chain(
            items
                .into_iter()
                .map(|(task_id, item_id)| Found::Item { task_id, item_id }),
        );
        let whole = |found: &Found| match found {
            Found::Task(task_id) => *task_id == id,
            Found::Item { item_id, .. } => *item_id == id,
        };
        match graph::only(found.collect(), whole) {
            Ok(Some(found)) => Ok(found),
            Ok(None) => bail!("no task or step {id} in this thread"),
            Err(()) => bail!("{id} fits more than one; give more of it"),
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

/// After the steps changed: the first open step starts when none runs, and a task on the
/// board is done exactly when it has steps and all of them are.
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
    // `done` and `status` are written together (a trigger refuses anything else), and
    // only between open and done: a task in another status is the plan's to move.
    sqlx::query(
        "WITH steps(all_done) AS (
             SELECT EXISTS (SELECT 1 FROM task_items WHERE task_id = ?2)
                AND NOT EXISTS (SELECT 1 FROM task_items WHERE task_id = ?2 AND done = 0))
         UPDATE tasks SET updated_at = ?1,
             done = CASE WHEN status NOT IN ('open', 'done') THEN done
                         ELSE (SELECT all_done FROM steps) END,
             status = CASE WHEN status NOT IN ('open', 'done') THEN status
                           WHEN (SELECT all_done FROM steps) THEN 'done' ELSE 'open' END
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
mod tests;
