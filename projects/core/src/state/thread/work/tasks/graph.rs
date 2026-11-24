//! The plan as a graph: the plan a thread belongs to, writing tasks with their
//! dependencies, the cycle check, what is ready, and the blocks behind a failed task.
//!
//! Every query here names the plan's root thread, found from the thread that asks, so a
//! thread reaches the plan of its own chat and no other.

use std::collections::HashMap;

use dusk_protocol::{config::ModelTier, model::Effort};
use sqlx::{Sqlite, Transaction};

use crate::state::thread::threads::chat_of;
use uuid::Uuid;

use super::{
    State, barrier,
    error::PlanError,
    plan::{
        MAX_DEPENDENCIES, MAX_KEY, MAX_OPEN_TASKS, MAX_TASKS_AN_ADD, NewTask, PlanStatus, PlanTask,
        TaskEdit, capped, check_texts, decode, select_tasks,
    },
    timestamp_ms,
};

pub(super) type Tx<'a> = Transaction<'a, Sqlite>;

/// The longest text taken for an id; a UUID is 36.
const MAX_ID: usize = 64;

// T1 and L1 are the callers; nothing in the tree is yet.
#[allow(dead_code)]
impl State {
    /// Adds tasks to the plan of the thread's chat, all or none. A task may depend on
    /// another of the same call by its key, or on a task of the plan by its id.
    pub async fn add_plan_tasks(
        &self,
        thread: &str,
        tasks: &[NewTask],
    ) -> Result<Vec<PlanTask>, PlanError> {
        if tasks.is_empty() {
            return Err(PlanError::Empty("the list of tasks"));
        }
        if tasks.len() > MAX_TASKS_AN_ADD {
            return Err(PlanError::TooMany {
                what: "tasks in one add",
                max: MAX_TASKS_AN_ADD,
            });
        }
        let mut keys: HashMap<&str, usize> = HashMap::new();
        for (index, task) in tasks.iter().enumerate() {
            check_texts(
                Some(&task.description),
                task.role.as_deref(),
                Some(&task.phase),
                Some(&task.acceptance),
                Some(&task.brief),
            )?;
            check_dependency_count(&task.after)?;
            let Some(key) = task.key.as_deref() else {
                continue;
            };
            if key.is_empty() {
                return Err(PlanError::Empty("a task's key"));
            }
            capped("a task's key", key, MAX_KEY)?;
            if keys.insert(key, index).is_some() {
                return Err(PlanError::DuplicateKey(key.to_owned()));
            }
        }
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE thread_id = ? AND status != 'done'",
        )
        .bind(&root)
        .fetch_one(&mut *transaction)
        .await?;
        if usize::try_from(open).unwrap_or(usize::MAX) + tasks.len() > MAX_OPEN_TASKS {
            return Err(PlanError::TooMany {
                what: "tasks not done in the plan",
                max: MAX_OPEN_TASKS,
            });
        }
        let ids: Vec<String> = tasks.iter().map(|_| Uuid::now_v7().to_string()).collect();
        for (task, task_id) in tasks.iter().zip(&ids) {
            let mut depends_on: Vec<String> = Vec::new();
            for after in &task.after {
                let dependency = match keys.get(after.as_str()) {
                    Some(index) => ids[*index].clone(),
                    None => dependency(&mut transaction, &root, after).await?,
                };
                if !depends_on.contains(&dependency) {
                    depends_on.push(dependency);
                }
            }
            sqlx::query(
                "INSERT INTO tasks (task_id, thread_id, description, depends_on, role, phase,
                     acceptance, brief, gate, files, tier, effort, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(task_id)
            .bind(&root)
            .bind(task.description.trim())
            .bind(serde_json::to_string(&depends_on)?)
            .bind(&task.role)
            .bind(&task.phase)
            .bind(&task.acceptance)
            .bind(&task.brief)
            .bind(task.gate.as_ref().map(serde_json::to_string).transpose()?)
            .bind(serde_json::to_string(task.files.entries())?)
            .bind(task.tier.map(ModelTier::name))
            .bind(task.effort.map(Effort::name))
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
        for task_id in &ids {
            refuse_cycle(&mut transaction, &root, task_id).await?;
        }
        settle_blocks(&mut transaction, &root, now).await?;
        let mut added = Vec::with_capacity(ids.len());
        for task_id in &ids {
            added.push(load(&mut transaction, &root, task_id).await?);
        }
        transaction.commit().await?;
        Ok(added)
    }

    /// Changes a task nobody has started: one that is open, or blocked only because a
    /// task it depends on failed. A member that lost it is not given it back: it worked
    /// from the old card.
    pub async fn edit_plan_task(
        &self,
        thread: &str,
        task: &str,
        edit: &TaskEdit,
    ) -> Result<PlanTask, PlanError> {
        check_texts(
            edit.description.as_deref(),
            edit.role.as_ref().and_then(|role| role.as_deref()),
            edit.phase.as_deref(),
            edit.acceptance.as_deref(),
            edit.brief.as_deref(),
        )?;
        if let Some(after) = &edit.after {
            check_dependency_count(after)?;
        }
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let found = load(&mut transaction, &root, &task_id).await?;
        let untouched = found.status == PlanStatus::Open
            || (found.status == PlanStatus::Blocked && found.owner_thread_id.is_none());
        if !untouched {
            return Err(PlanError::Illegal {
                task: task_id,
                from: found.status,
                to: PlanStatus::Open,
            });
        }
        let mut depends_on = found.depends_on;
        if let Some(after) = &edit.after {
            depends_on.clear();
            for after in after {
                let dependency = dependency(&mut transaction, &root, after).await?;
                if !depends_on.contains(&dependency) {
                    depends_on.push(dependency);
                }
            }
        }
        let gate = edit.gate.as_ref().unwrap_or(&found.gate);
        sqlx::query(
            "UPDATE tasks SET description = ?, depends_on = ?, role = ?, phase = ?,
                 acceptance = ?, brief = ?, gate = ?, files = ?, tier = ?, effort = ?,
                 updated_at = ?, last_owner_thread_id = NULL,
                 skipped = CASE status WHEN 'open' THEN 0 ELSE skipped END
             WHERE task_id = ? AND thread_id = ?",
        )
        .bind(
            edit.description
                .as_deref()
                .map_or(found.description.as_str(), str::trim),
        )
        .bind(serde_json::to_string(&depends_on)?)
        .bind(edit.role.as_ref().unwrap_or(&found.role))
        .bind(edit.phase.as_ref().unwrap_or(&found.phase))
        .bind(edit.acceptance.as_ref().unwrap_or(&found.acceptance))
        .bind(edit.brief.as_ref().unwrap_or(&found.brief))
        .bind(gate.as_ref().map(serde_json::to_string).transpose()?)
        .bind(serde_json::to_string(
            edit.files.as_ref().unwrap_or(&found.files).entries(),
        )?)
        .bind(edit.tier.or(found.tier).map(ModelTier::name))
        .bind(edit.effort.or(found.effort).map(Effort::name))
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?;
        refuse_cycle(&mut transaction, &root, &task_id).await?;
        settle_blocks(&mut transaction, &root, now).await?;
        let edited = load(&mut transaction, &root, &task_id).await?;
        transaction.commit().await?;
        Ok(edited)
    }

    /// The plan of the thread's chat, oldest task first.
    pub async fn plan(&self, thread: &str) -> Result<Vec<PlanTask>, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        sqlx::query(select_tasks!(
            "WHERE t.thread_id = ? ORDER BY t.created_at, t.task_id"
        ))
        .bind(&root)
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(decode)
        .collect()
    }

    /// One task of the plan, by its id or the end of it.
    pub async fn plan_task(&self, thread: &str, task: &str) -> Result<PlanTask, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        load(&mut transaction, &root, &task_id).await
    }

    /// The open tasks whose every dependency is done and that stand in the current wave
    /// (`barrier`), by phase label, oldest first. A task the lead skipped is never ready.
    pub async fn ready_tasks(&self, thread: &str) -> Result<Vec<PlanTask>, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        // A dependency whose row is gone counts as not done.
        let ready: Vec<PlanTask> = sqlx::query(select_tasks!(
            "WHERE t.thread_id = ? AND t.status = 'open' AND t.skipped = 0
               AND NOT EXISTS (
                   SELECT 1 FROM json_each(t.depends_on) d
                   LEFT JOIN tasks dep ON dep.task_id = d.value AND dep.thread_id = t.thread_id
                   WHERE dep.status IS NOT 'done')
             ORDER BY t.phase, t.created_at, t.task_id"
        ))
        .bind(&root)
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(decode)
        .collect::<Result<_, _>>()?;
        if ready.is_empty() {
            return Ok(ready);
        }
        let plan: Vec<PlanTask> = sqlx::query(select_tasks!("WHERE t.thread_id = ?"))
            .bind(&root)
            .fetch_all(&mut *transaction)
            .await?
            .iter()
            .map(decode)
            .collect::<Result<_, _>>()?;
        let waves = barrier::waves(&plan);
        let current = barrier::current_wave(&plan);
        let wave_of = |task_id: &str| {
            plan.iter()
                .zip(&waves)
                .find(|(task, _)| task.task_id == task_id)
                .map(|(_, wave)| *wave)
        };
        Ok(ready
            .into_iter()
            .filter(|task| current.is_some() && wave_of(&task.task_id) == current)
            .collect())
    }
}

fn check_dependency_count(after: &[String]) -> Result<(), PlanError> {
    if after.len() > MAX_DEPENDENCIES {
        return Err(PlanError::TooMany {
            what: "tasks one task depends on",
            max: MAX_DEPENDENCIES,
        });
    }
    Ok(())
}

/// The root thread of the chat `thread` is in: where its plan and its team are kept.
pub(in crate::state::thread::work) async fn root_of(
    transaction: &mut Tx<'_>,
    thread: &str,
) -> Result<String, PlanError> {
    chat_of(&mut **transaction, thread)
        .await?
        .ok_or_else(|| PlanError::NoSuchThread(thread.to_owned()))
}

/// An id as given, or the end of one: trimmed, without the `#` the board prints, with its
/// length in characters, lowercased as ids are made. `None` when it is shorter than 4 or
/// longer than `MAX_ID`.
pub(super) fn id_end(id: &str) -> Option<(String, i64)> {
    let id = id.trim().trim_start_matches('#').to_ascii_lowercase();
    let length = id.chars().count();
    (4..=MAX_ID)
        .contains(&length)
        .then(|| (id, i64::try_from(length).unwrap_or(i64::MAX)))
}

/// `$column` is the id bound as `?2`, or ends with it, `?3` characters long. Compared,
/// never matched as a pattern: `%` and `_` in an id are just characters.
macro_rules! fits_id {
    ($column:literal) => {
        concat!("(", $column, " = ?2 OR substr(", $column, ", -?3) = ?2)")
    };
}
pub(super) use fits_id;

/// What fits an id end: one, or the whole id when more than one ends with it.
pub(super) fn only<T>(mut found: Vec<T>, whole: impl Fn(&T) -> bool) -> Result<Option<T>, ()> {
    if found.len() > 1 && found.iter().any(&whole) {
        found.retain(whole);
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(()),
    }
}

/// The id of a task of the plan, from the id or the end of it when only one fits.
pub(super) async fn locate(
    transaction: &mut Tx<'_>,
    root: &str,
    id: &str,
) -> Result<String, PlanError> {
    let Some((id, length)) = id_end(id) else {
        return Err(PlanError::NoSuchTask(
            id.trim().trim_start_matches('#').to_owned(),
        ));
    };
    let found: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT task_id FROM tasks WHERE thread_id = ?1 AND ",
        fits_id!("task_id")
    ))
    .bind(root)
    .bind(&id)
    .bind(length)
    .fetch_all(&mut **transaction)
    .await?;
    match only(found, |task_id| *task_id == id) {
        Ok(Some(task_id)) => Ok(task_id),
        Ok(None) => Err(PlanError::NoSuchTask(id.clone())),
        Err(()) => Err(PlanError::Ambiguous(id)),
    }
}

/// A dependency must be a task of the same plan.
async fn dependency(transaction: &mut Tx<'_>, root: &str, id: &str) -> Result<String, PlanError> {
    match locate(transaction, root, id).await {
        Err(PlanError::NoSuchTask(id)) => Err(PlanError::UnknownDependency(id)),
        found => found,
    }
}

pub(super) async fn load(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: &str,
) -> Result<PlanTask, PlanError> {
    let row = sqlx::query(select_tasks!("WHERE t.task_id = ? AND t.thread_id = ?"))
        .bind(task_id)
        .bind(root)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| PlanError::NoSuchTask(task_id.to_owned()))?;
    decode(&row)
}

/// Refuses a written task that reaches itself through `depends_on`. Dropping the
/// transaction undoes the write.
async fn refuse_cycle(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: &str,
) -> Result<(), PlanError> {
    let reaches_itself: bool = sqlx::query_scalar(
        "WITH RECURSIVE reach(id) AS (
            SELECT d.value FROM tasks t, json_each(t.depends_on) d
            WHERE t.task_id = ?1 AND t.thread_id = ?2
            UNION
            SELECT d.value FROM reach JOIN tasks t ON t.task_id = reach.id AND t.thread_id = ?2,
                 json_each(t.depends_on) d
         ) SELECT EXISTS (SELECT 1 FROM reach WHERE id = ?1)",
    )
    .bind(task_id)
    .bind(root)
    .fetch_one(&mut **transaction)
    .await?;
    if !reaches_itself {
        return Ok(());
    }
    // Only to say which tasks make the loop; a plan is small.
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT task_id, depends_on FROM tasks WHERE thread_id = ?")
            .bind(root)
            .fetch_all(&mut **transaction)
            .await?;
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (id, depends_on) in rows {
        edges.insert(id, serde_json::from_str(&depends_on)?);
    }
    Err(PlanError::Cycle(loop_from(task_id, &edges)))
}

/// The shortest way from `start` back to itself, both ends included.
fn loop_from(start: &str, edges: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut came_from: HashMap<&str, &str> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        for next in edges.get(at).into_iter().flatten() {
            if next == start {
                let mut path = vec![start.to_owned(), at.to_owned()];
                let mut at = at;
                while at != start
                    && let Some(before) = came_from.get(at)
                {
                    path.push((*before).to_owned());
                    at = before;
                }
                // Built from the end: the closing start first, then back to the start.
                path.reverse();
                return path;
            }
            if !came_from.contains_key(next.as_str()) {
                came_from.insert(next, at);
                queue.push_back(next);
            }
        }
    }
    Vec::new()
}

/// For every task of the plan that has not started, a failed task it stands behind.
macro_rules! behind {
    ($update:literal) => {
        concat!(
            "WITH RECURSIVE up(task, dep) AS (
                SELECT t.task_id, d.value FROM tasks t, json_each(t.depends_on) d
                WHERE t.thread_id = ?1 AND t.status IN ('open', 'blocked')
                UNION
                SELECT up.task, d.value
                FROM up JOIN tasks m ON m.task_id = up.dep AND m.thread_id = ?1
                     AND m.status != 'done', json_each(m.depends_on) d
             ), behind(task, dep) AS (
                SELECT up.task, MIN(up.dep) FROM up
                JOIN tasks f ON f.task_id = up.dep AND f.thread_id = ?1 AND f.status = 'failed'
                GROUP BY up.task
             ) ",
            $update
        )
    };
}

/// A task failed: its open dependants, through the graph, are blocked on it. A task left
/// `failed`: those blocked on it open again, unless another failed task is before them.
/// Run after any change that makes or unmakes a failed task or a dependency; it changes
/// nothing when nothing is out of place. Only tasks without an owner are touched: a
/// member's blocked task is its own.
pub(super) async fn settle_blocks(
    transaction: &mut Tx<'_>,
    root: &str,
    now: i64,
) -> Result<(), PlanError> {
    sqlx::query(behind!(
        "UPDATE tasks SET status = 'blocked', updated_at = ?2,
             waits_on = 'task:' || (SELECT dep FROM behind WHERE task = tasks.task_id)
         WHERE thread_id = ?1 AND owner_thread_id IS NULL
           AND task_id IN (SELECT task FROM behind)
           AND (status = 'open' OR (status = 'blocked' AND substr(waits_on, 1, 5) = 'task:'
                AND waits_on != 'task:' || (SELECT dep FROM behind WHERE task = tasks.task_id)))"
    ))
    .bind(root)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(behind!(
        "UPDATE tasks SET status = 'open', waits_on = NULL, updated_at = ?2
         WHERE thread_id = ?1 AND owner_thread_id IS NULL AND status = 'blocked'
           AND substr(waits_on, 1, 5) = 'task:'
           AND task_id NOT IN (SELECT task FROM behind)"
    ))
    .bind(root)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
