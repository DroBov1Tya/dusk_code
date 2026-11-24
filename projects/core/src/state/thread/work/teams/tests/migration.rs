//! Migrations 0037 and 0050 on a database with a plan or a team, and every CHECK of the
//! table.

use super::*;

/// A database as the version before `before` left it: every older migration and no newer.
async fn old_database(path: &std::path::Path, before: i64) -> sqlx::SqlitePool {
    let older: Vec<_> = sqlx::migrate!("src/state/db/migrations")
        .iter()
        .filter(|migration| migration.version < before)
        .cloned()
        .collect();
    assert_eq!(
        older.last().map(|migration| migration.version),
        Some(before - 1)
    );
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
async fn the_migration_keeps_a_plan_and_its_checks_hold() {
    let path = temp_dir("team_migration").join("state.db");
    let pool = old_database(&path, 37).await;
    sqlx::raw_sql(
        "INSERT INTO threads (id, source, created_at) VALUES ('lead', 'test', 1);
         INSERT INTO threads (id, source, created_at, parent_thread_id, agent, name)
         VALUES ('m0', 'agent', 1, 'lead', 'general', 'm0');
         INSERT INTO tasks (task_id, thread_id, description, item_ids, done, status,
                            created_at, updated_at)
         VALUES ('task-open-0001', 'lead', 'open one', '[]', 0, 'open', 1, 1),
                ('task-done-0002', 'lead', 'done one', '[]', 1, 'done', 2, 2);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let state = State::open(&path).await.unwrap();
    let counts = state.team_counts("m0").await.unwrap();
    assert_eq!((counts.total, counts.ready, counts.done), (2, 1, 1));
    state.start_team("m0", plain(), START).await.unwrap();
    sqlx::raw_sql("DELETE FROM teams; INSERT INTO threads (id, source, created_at) VALUES ('two', 'test', 1);")
        .execute(&state.pool)
        .await
        .unwrap();

    // (thread_id, status, ended_why, at_once, worktrees, base_commit, started_at, ended_at)
    let hash = "a".repeat(40);
    let refused = [
        "'lead', 'paused', NULL, 3, 0, NULL, 1000, NULL".to_owned(),
        "'lead', 'ended', 'bored', 3, 0, NULL, 1000, 1500".to_owned(),
        "'lead', 'running', NULL, 0, 0, NULL, 1000, NULL".to_owned(),
        "'lead', 'running', NULL, 9, 0, NULL, 1000, NULL".to_owned(),
        "'lead', 'running', NULL, 3, 2, NULL, 1000, NULL".to_owned(),
        "'lead', 'running', NULL, 3, 1, 'abc', 1000, NULL".to_owned(),
        format!("'lead', 'running', NULL, 3, 0, '{hash}', 1000, NULL"),
        "'lead', 'running', NULL, 3, 0, NULL, -1, NULL".to_owned(),
        "'lead', 'running', 'done', 3, 0, NULL, 1000, NULL".to_owned(),
        "'lead', 'running', NULL, 3, 0, NULL, 1000, 1500".to_owned(),
        "'lead', 'ended', NULL, 3, 0, NULL, 1000, 1500".to_owned(),
        "'lead', 'ended', 'done', 3, 0, NULL, 1000, NULL".to_owned(),
        "'ghost', 'running', NULL, 3, 0, NULL, 1000, NULL".to_owned(),
    ];
    let insert = |values: &str| {
        format!(
            "INSERT INTO teams (thread_id, status, ended_why, at_once, worktrees, base_commit,
                 started_at, ended_at) VALUES ({values})"
        )
    };
    for values in &refused {
        let why = if values.starts_with("'ghost'") {
            "FOREIGN KEY"
        } else {
            "CHECK"
        };
        let error = sqlx::raw_sql(sqlx::AssertSqlSafe(insert(values)))
            .execute(&state.pool)
            .await
            .expect_err(values)
            .to_string();
        assert!(error.contains(why), "{values}: {error}");
    }
    let edge = "'lead', 'running', NULL, 8, 0, NULL, 0, NULL";
    for values in [
        "'lead', 'running', NULL, 3, 0, NULL, 1000, NULL".to_owned(),
        format!("'two', 'ended', 'quiet', 1, 1, '{hash}', 1000, 900"),
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(insert(&values)))
            .execute(&state.pool)
            .await
            .unwrap_or_else(|error| panic!("{values}: {error}"));
    }
    let error = sqlx::raw_sql(sqlx::AssertSqlSafe(insert(edge)))
        .execute(&state.pool)
        .await
        .expect_err("one team a chat")
        .to_string();
    assert!(error.contains("UNIQUE"), "{error}");
    sqlx::raw_sql("DELETE FROM teams WHERE thread_id = 'lead'")
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(insert(edge)))
        .execute(&state.pool)
        .await
        .expect("the edges of every CHECK are taken");
    state.close().await;
}

/// A team of a core that had budgets: 0050 drops the budget, keeps the rest of the row,
/// and a run that ended on a budget still reads with that reason.
#[tokio::test]
async fn a_team_from_before_0050_loads_without_its_budget() {
    let path = temp_dir("team_no_budget").join("state.db");
    let pool = old_database(&path, 50).await;
    sqlx::raw_sql(
        "INSERT INTO threads (id, source, created_at) VALUES ('lead', 'test', 1);
         INSERT INTO threads (id, source, created_at) VALUES ('two', 'test', 1);
         INSERT INTO teams (thread_id, status, ended_why, at_once, max_steps, max_tokens,
                            deadline, worktrees, base_commit, started_at, ended_at)
         VALUES ('lead', 'ended', 'tokens', 2, 400, 4000000, 7201000, 0, NULL, 1000, 5000),
                ('two', 'running', NULL, 3, 400, 4000000, 7201000, 0, NULL, 1000, NULL);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let state = State::open(&path).await.unwrap();
    let ended = state.team_run(LEAD).await.unwrap().unwrap();
    assert_eq!(ended.status, TeamStatus::Ended(TeamEnd::Tokens));
    assert_eq!(
        (ended.at_once, ended.started_at, ended.ended_at),
        (2, 1000, Some(5000))
    );
    let running = state.team_run("two").await.unwrap().unwrap();
    assert_eq!((running.status, running.at_once), (TeamStatus::Running, 3));
    // Nothing of the old budget bounds it.
    assert_eq!(
        state.team_check("two", TeamAsk::Claim).await.unwrap(),
        TeamCheck::Go
    );
    // A new run starts in the row the old one left.
    state.start_team(LEAD, plain(), 9_000).await.unwrap();
    state.close().await;
}
