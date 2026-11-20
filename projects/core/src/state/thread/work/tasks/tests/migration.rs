//! Migration 0036 on a database that has tasks already, and its CHECKs and triggers on
//! the bundled SQLite, row by row.

use std::borrow::Cow;

use sqlx::{
    ConnectOptions, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use super::kit::{LEAD, done_and_status, team};
use crate::{
    state::{PlanStatus, State},
    testing::temp_dir,
};

/// A database as the version before 0036 left it: every older migration and no newer.
async fn old_database(path: &std::path::Path) -> SqlitePool {
    let all = sqlx::migrate!("src/state/db/migrations");
    let older: Vec<_> = all
        .iter()
        .filter(|migration| migration.version < 36)
        .cloned()
        .collect();
    assert_eq!(older.last().map(|migration| migration.version), Some(35));
    let migrator = Migrator {
        migrations: Cow::Owned(older),
        ..Migrator::DEFAULT
    };
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    migrator.run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn old_tasks_get_their_status_from_done() {
    let path = temp_dir("plan_migration").join("state.db");
    let pool = old_database(&path).await;
    sqlx::raw_sql(
        "INSERT INTO threads (id, source, created_at) VALUES ('mine', 'test', 1);
         INSERT INTO tasks (task_id, thread_id, description, item_ids, done, created_at, updated_at)
         VALUES ('task-open-0001', 'mine', 'open one', '[\"item-1\",\"item-2\"]', 0, 1, 1),
                ('task-done-0002', 'mine', 'done one', '[]', 1, 2, 2);
         INSERT INTO task_items (item_id, task_id, description, done, created_at, updated_at,
                                 started_at, finished_at)
         VALUES ('item-1', 'task-open-0001', 'read', 1, 1, 1, 1, 1),
                ('item-2', 'task-open-0001', 'write', 0, 1, 1, 1, NULL);
         INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at)
         VALUES ('turn-1', 'mine', 1, 'interrupted', 1, 1);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let state = State::open(&path).await.unwrap();
    let plan = state.plan("mine").await.unwrap();
    assert_eq!(plan.len(), 2);
    assert_eq!(
        (plan[0].status, plan[1].status),
        (PlanStatus::Open, PlanStatus::Done)
    );
    for task in &plan {
        assert!(task.depends_on.is_empty() && task.files.entries().is_empty());
        assert_eq!((task.attempts, task.reworks), (0, 0));
        assert_eq!(
            (&task.owner_thread_id, task.lease_until, &task.gate),
            (&None, None, &None)
        );
        assert_eq!((task.phase.as_str(), task.note.as_str()), ("", ""));
    }
    assert_eq!(
        done_and_status(&state).await,
        [(0, "open".to_owned()), (1, "done".to_owned())]
    );
    let task_of_turn: Option<String> = sqlx::query_scalar("SELECT task_id FROM turns")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(task_of_turn, None);

    // The board goes on as before, and `done` follows `status` both ways.
    let board = state.thread_tasks("mine").await.unwrap();
    assert!(!board[0].done && board[1].done && board[0].items[0].done);
    let closed = state.complete("mine", "item-2").await.unwrap();
    assert!(closed.done);
    let reopened = state
        .add_task_items("mine", "task-done-0002", &["more".to_owned()])
        .await
        .unwrap();
    assert!(!reopened.done);
    assert_eq!(
        done_and_status(&state).await,
        [(1, "done".to_owned()), (0, "open".to_owned())]
    );
    // And the old rows are tasks of a plan like any other.
    state
        .claim_task("mine", "task-done-0002", super::kit::claim("mine"))
        .await
        .unwrap();
    state.close().await;
    // Opening it again changes nothing: the migration ran once.
    let state = State::open(&path).await.unwrap();
    assert_eq!(
        state.plan("mine").await.unwrap()[1].status,
        PlanStatus::Claimed
    );
    state.close().await;
}

async fn refused(state: &State, sql: &str, why: &str) {
    let error = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&state.pool)
        .await
        .expect_err(sql)
        .to_string();
    assert!(error.contains(why), "{sql}: {error}");
}

#[tokio::test]
async fn done_and_status_are_written_together_or_not_at_all() {
    let state = team().await;
    let task = state.add_task(LEAD, "a task", &[]).await.unwrap();
    let disagree = "done and status disagree";
    refused(&state, "UPDATE tasks SET done = 1", disagree).await;
    refused(&state, "UPDATE tasks SET status = 'done'", disagree).await;
    sqlx::raw_sql("UPDATE tasks SET done = 1, status = 'done'")
        .execute(&state.pool)
        .await
        .unwrap();
    refused(&state, "UPDATE tasks SET done = 0", disagree).await;
    refused(&state, "UPDATE tasks SET status = 'open'", disagree).await;
    refused(&state, "UPDATE tasks SET status = 'failed'", disagree).await;
    sqlx::raw_sql("UPDATE tasks SET done = 0, status = 'review'")
        .execute(&state.pool)
        .await
        .unwrap();
    // A new row is held to the same.
    let insert = |more: &str, values: &str| {
        format!(
            "INSERT INTO tasks (task_id, thread_id, description, created_at, updated_at{more})
             VALUES ('new', 'lead', 'x', 1, 1{values})"
        )
    };
    refused(&state, &insert(", done", ", 1"), disagree).await;
    refused(&state, &insert(", status", ", 'done'"), disagree).await;
    refused(
        &state,
        &insert(", status", ", 'running'"),
        "owner and a lease",
    )
    .await;
    refused(&state, &insert(", lease_until", ", 5"), "owner and a lease").await;
    refused(
        &state,
        &insert(", owner_thread_id", ", 'm0'"),
        "an open task has no owner",
    )
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(insert(", done, status", ", 1, 'done'")))
        .execute(&state.pool)
        .await
        .unwrap();
    // The board's own writes keep the two together.
    sqlx::raw_sql("UPDATE tasks SET done = 0, status = 'open'")
        .execute(&state.pool)
        .await
        .unwrap();
    assert!(state.complete(LEAD, &task.task_id).await.unwrap().done);
}

#[tokio::test]
async fn a_lease_and_an_owner_follow_the_status_row_by_row() {
    let state = team().await;
    state.add_task(LEAD, "a task", &[]).await.unwrap();
    for status in PlanStatus::ALL {
        for owner in [None, Some("m0")] {
            for lease in [None, Some(5_i64)] {
                let at_work = matches!(status, PlanStatus::Claimed | PlanStatus::Running);
                let allowed = if at_work {
                    owner.is_some() && lease.is_some()
                } else {
                    lease.is_none() && (status != PlanStatus::Open || owner.is_none())
                };
                let written = sqlx::query(
                    "UPDATE tasks SET status = ?1, done = (?1 = 'done'), owner_thread_id = ?2,
                         lease_until = ?3",
                )
                .bind(status.as_str())
                .bind(owner)
                .bind(lease)
                .execute(&state.pool)
                .await;
                assert_eq!(written.is_ok(), allowed, "{status:?} {owner:?} {lease:?}");
            }
        }
    }
    // One task at work for a thread, by the index.
    state.add_task(LEAD, "another", &[]).await.unwrap();
    let both = sqlx::raw_sql(
        "UPDATE tasks SET status = 'running', done = 0, owner_thread_id = 'm0', lease_until = 5",
    )
    .execute(&state.pool)
    .await
    .unwrap_err();
    assert!(both.to_string().contains("UNIQUE"), "{both}");
}

#[tokio::test]
async fn the_new_columns_refuse_what_their_checks_forbid() {
    let state = team().await;
    state.add_task(LEAD, "a task", &[]).await.unwrap();
    for bad in [
        "status = 'started'",
        "depends_on = '{}'",
        "depends_on = 'not json'",
        "files = '\"src\"'",
        "role = ''",
        "role = printf('%.*c', 129, 'x')",
        "phase = printf('%.*c', 33, 'x')",
        "acceptance = printf('%.*c', 601, 'x')",
        "brief = printf('%.*c', 1201, 'x')",
        "note = printf('%.*c', 601, 'x')",
        "gate = '{'",
        "checkpoint = 'abc'",
        "attempts = -1",
        "reworks = -1",
        "owner_thread_id = 'no such thread', status = 'blocked'",
    ] {
        let sql = format!("UPDATE tasks SET {bad}");
        let written = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&state.pool)
            .await;
        assert!(written.is_err(), "{bad}");
    }
    let turn = sqlx::raw_sql(
        "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at,
                            task_id) VALUES ('t', 'lead', 1, 'interrupted', 1, 1, 'no such task')",
    )
    .execute(&state.pool)
    .await;
    assert!(turn.is_err(), "a turn points at a task that exists");
    for good in [
        "role = printf('%.*c', 128, 'x'), phase = printf('%.*c', 32, 'x')",
        "note = printf('%.*c', 600, 'x'), checkpoint = printf('%.*c', 40, 'a')",
        "gate = '{\"kind\":\"user_confirmation\"}', files = '[\"src/\"]'",
    ] {
        let sql = format!("UPDATE tasks SET {good}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&state.pool)
            .await
            .unwrap();
    }
}
