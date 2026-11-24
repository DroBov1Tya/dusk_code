//! A plan's tasks as clients see them: the one mapping from the store's `PlanTask` to the
//! protocol's `Task`, the numbers joined to it, and the page of the plan for the strip.
//!
//! Steps, tokens and changes are never stored on a task: they are counted from
//! `model_usage` and `changes` through `turns.task_id`, in one query for all the tasks
//! shown, so a page costs four queries however many tasks it holds.

use std::collections::HashMap;

use dusk_protocol::thread::{
    PLAN_PAGE_DONE, PlanCounts, PlanPage, Task, TaskItem, TaskOwner, TaskStatus,
};
use sqlx::{Row, SqliteConnection};

use super::{
    State, WaitsOn,
    error::PlanError,
    graph::{load, locate, root_of},
    is_done,
    plan::{MAX_OPEN_TASKS, PlanStatus, PlanTask, decode, select_tasks},
};

/// What a task's turns used, as counted.
#[derive(Debug, Clone, Copy, Default)]
struct Used {
    steps: u32,
    tokens: u64,
    changes: u32,
}

impl State {
    /// The page of the plan of the thread's chat: every task not done, oldest first, then
    /// up to `limit` done ones (held to `PLAN_PAGE_DONE`) older than the task
    /// `done_before`, newest first. "Older" is by creation, which an id of the plan fixes
    /// for good, so a page never moves under the cursor. `counts` is the whole plan's.
    pub async fn plan_page(
        &self,
        thread: &str,
        done_before: Option<&str>,
        limit: u32,
    ) -> Result<PlanPage, PlanError> {
        let limit = limit.clamp(1, PLAN_PAGE_DONE);
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        // The cursor must be a task of this plan, so another chat's id finds nothing.
        let cursor = match done_before {
            Some(id) => Some(locate(&mut transaction, &root, id).await?),
            None => None,
        };
        let mut tasks: Vec<PlanTask> = sqlx::query(select_tasks!(
            "WHERE t.thread_id = ?1 AND t.status != 'done'
             ORDER BY t.created_at, t.task_id LIMIT ?2"
        ))
        .bind(&root)
        .bind(i64::try_from(MAX_OPEN_TASKS).unwrap_or(i64::MAX))
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(decode)
        .collect::<Result<_, _>>()?;
        // One more than the page shows tells whether another page is left.
        let mut done: Vec<PlanTask> = sqlx::query(select_tasks!(
            "WHERE t.thread_id = ?1 AND t.status = 'done'
               AND (?3 IS NULL OR (t.created_at, t.task_id) < (
                   SELECT created_at, task_id FROM tasks
                   WHERE task_id = ?3 AND thread_id = ?1))
             ORDER BY t.created_at DESC, t.task_id DESC LIMIT ?2"
        ))
        .bind(&root)
        .bind(i64::from(limit) + 1)
        .bind(cursor.as_deref())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(decode)
        .collect::<Result<_, _>>()?;
        let next = if done.len() > limit as usize {
            done.truncate(limit as usize);
            done.last().map(|task| task.task_id.clone())
        } else {
            None
        };
        tasks.extend(done);
        let counts = counts(&mut transaction, &root).await?;
        Ok(PlanPage {
            tasks: present(&mut transaction, tasks).await?,
            next,
            counts,
        })
    }

    /// One task of the thread's plan as clients see it, by its id or the end of it.
    pub async fn plan_task_view(&self, thread: &str, task: &str) -> Result<Task, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let found = load(&mut transaction, &root, &task_id).await?;
        let mut shown = present(&mut transaction, vec![found]).await?;
        shown.pop().ok_or(PlanError::NoSuchTask(task_id))
    }

    /// A task the store just answered with, as clients see it.
    pub async fn present_task(&self, task: PlanTask) -> Result<Task, PlanError> {
        let mut connection = self.pool.acquire().await?;
        let mut shown = present(&mut connection, vec![task]).await?;
        shown.pop().ok_or(PlanError::NoSuchTask(String::new()))
    }
}

/// The lead's own tasks by status: the reviews and the critic the core adds are shown on
/// the plan, but "3 of 7 done" counts the work.
async fn counts(connection: &mut SqliteConnection, root: &str) -> Result<PlanCounts, PlanError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM tasks WHERE thread_id = ? AND duty = 'work' GROUP BY status",
    )
    .bind(root)
    .fetch_all(connection)
    .await?;
    let mut counts = PlanCounts::default();
    for (status, count) in rows {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        let status = PlanStatus::parse(&status)
            .ok_or_else(|| PlanError::Store(anyhow::anyhow!("unknown task status {status}")))?;
        match status {
            PlanStatus::Open => counts.open = count,
            PlanStatus::Claimed => counts.claimed = count,
            PlanStatus::Running => counts.running = count,
            PlanStatus::Blocked => counts.blocked = count,
            PlanStatus::Review => counts.review = count,
            PlanStatus::Done => counts.done = count,
            PlanStatus::Failed => counts.failed = count,
        }
    }
    Ok(counts)
}

/// The tasks as clients see them, in the order given.
pub(super) async fn present(
    connection: &mut SqliteConnection,
    tasks: Vec<PlanTask>,
) -> Result<Vec<Task>, PlanError> {
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
    let ids = serde_json::to_string(&ids)?;
    let mut items = items(connection, &ids).await?;
    let mut used = used(connection, &ids).await?;
    let owners: Vec<&str> = tasks
        .iter()
        .filter_map(|task| task.owner_thread_id.as_deref())
        .collect();
    let names = names(connection, &serde_json::to_string(&owners)?).await?;
    Ok(tasks
        .into_iter()
        .map(|task| {
            let items = items.remove(&task.task_id).unwrap_or_default();
            let used = used.remove(&task.task_id).unwrap_or_default();
            let owner = task.owner_thread_id.as_ref().map(|thread_id| TaskOwner {
                name: names.get(thread_id).unwrap_or(thread_id).clone(),
                thread_id: thread_id.clone(),
            });
            task_from(task, items, owner, used)
        })
        .collect())
}

/// The one mapping from a stored task to the protocol's: every field the store has and
/// the protocol names. `checkpoint`, `lease_until` and `gate_job` are the core's own and
/// stay out.
fn task_from(task: PlanTask, items: Vec<TaskItem>, owner: Option<TaskOwner>, used: Used) -> Task {
    Task {
        task_id: task.task_id,
        description: task.description,
        done: task.status == PlanStatus::Done,
        created_at: task.created_at,
        updated_at: task.updated_at,
        items,
        status: status(task.status),
        depends_on: task.depends_on,
        role: task.role,
        phase: task.phase,
        acceptance: task.acceptance,
        brief: task.brief,
        gate: task.gate,
        files: task.files.entries().to_vec(),
        owner,
        attempts: task.attempts,
        reworks: task.reworks,
        waits_on: task.waits_on.map(waits_on),
        note: task.note,
        model: task.model,
        steps: used.steps,
        tokens: used.tokens,
        changes: used.changes,
        result: task.result,
        duty: task.duty,
        review_of: task.review_of,
        skipped: task.skipped,
    }
}

fn status(status: PlanStatus) -> TaskStatus {
    match status {
        PlanStatus::Open => TaskStatus::Open,
        PlanStatus::Claimed => TaskStatus::Claimed,
        PlanStatus::Running => TaskStatus::Running,
        PlanStatus::Blocked => TaskStatus::Blocked,
        PlanStatus::Review => TaskStatus::Review,
        PlanStatus::Done => TaskStatus::Done,
        PlanStatus::Failed => TaskStatus::Failed,
    }
}

fn waits_on(waits: WaitsOn) -> dusk_protocol::thread::WaitsOn {
    use dusk_protocol::thread::WaitsOn as Shown;
    match waits {
        WaitsOn::Task(id) => Shown::Task(id),
        WaitsOn::Message(id) => Shown::Message(id),
        WaitsOn::Lead => Shown::Lead,
        WaitsOn::Review(id) => Shown::Review(id),
    }
}

/// The steps of each task, in order, by task id.
async fn items(
    connection: &mut SqliteConnection,
    ids: &str,
) -> Result<HashMap<String, Vec<TaskItem>>, PlanError> {
    let rows = sqlx::query(
        "SELECT t.task_id, i.item_id, i.description, i.done, i.created_at, i.updated_at,
                i.started_at, i.finished_at
         FROM tasks t, json_each(t.item_ids) j JOIN task_items i ON i.item_id = j.value
         WHERE t.task_id IN (SELECT value FROM json_each(?)) ORDER BY t.task_id, j.key",
    )
    .bind(ids)
    .fetch_all(connection)
    .await?;
    let mut by_task: HashMap<String, Vec<TaskItem>> = HashMap::new();
    for row in &rows {
        let item = TaskItem {
            item_id: row.try_get("item_id")?,
            description: row.try_get("description")?,
            done: is_done(row)?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
        };
        by_task
            .entry(row.try_get("task_id")?)
            .or_default()
            .push(item);
    }
    Ok(by_task)
}

/// Steps (the calls that answer), tokens (every call) and files changed (each path
/// once) over the turns that worked on each task, for a JSON array of task ids.
/// `turns_task` finds the turns, `model_usage_turn` and `changes_turn` their rows.
macro_rules! used {
    () => {
        "SELECT j.value AS task_id,
             (SELECT COUNT(*) FROM turns tr JOIN model_usage u ON u.turn_id = tr.turn_id
              WHERE tr.task_id = j.value AND u.purpose = 'answer') AS steps,
             (SELECT COALESCE(SUM(u.prompt_tokens + u.completion_tokens), 0)
              FROM turns tr JOIN model_usage u ON u.turn_id = tr.turn_id
              WHERE tr.task_id = j.value) AS tokens,
             (SELECT COUNT(DISTINCT c.project || char(0) || c.path)
              FROM turns tr JOIN changes c ON c.turn_id = tr.turn_id
              WHERE tr.task_id = j.value) AS changes
         FROM json_each(?) j"
    };
}

async fn used(
    connection: &mut SqliteConnection,
    ids: &str,
) -> Result<HashMap<String, Used>, PlanError> {
    let rows = sqlx::query(used!()).bind(ids).fetch_all(connection).await?;
    let mut by_task = HashMap::new();
    for row in &rows {
        let steps: i64 = row.try_get("steps")?;
        let tokens: i64 = row.try_get("tokens")?;
        let changes: i64 = row.try_get("changes")?;
        by_task.insert(
            row.try_get("task_id")?,
            Used {
                steps: u32::try_from(steps).unwrap_or(u32::MAX),
                tokens: u64::try_from(tokens).unwrap_or(0),
                changes: u32::try_from(changes).unwrap_or(u32::MAX),
            },
        );
    }
    Ok(by_task)
}

/// The display name of each thread of the list, its id when it has none.
async fn names(
    connection: &mut SqliteConnection,
    ids: &str,
) -> Result<HashMap<String, String>, PlanError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, COALESCE(NULLIF(name, ''), id) FROM threads
         WHERE id IN (SELECT value FROM json_each(?))",
    )
    .bind(ids)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn the_plan_reads_what_tasks_used_by_index() {
        let state = crate::testing::temp_state("task_used_plan").await;
        let plan =
            crate::testing::query_plan(&state, concat!("EXPLAIN QUERY PLAN ", used!())).await;
        assert!(!plan.contains("SCAN u"), "{plan}");
        assert!(!plan.contains("AUTOMATIC"), "{plan}");
        assert!(plan.contains("model_usage_turn"), "{plan}");
    }
}
