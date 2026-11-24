//! A chat's standing goal, kept across restarts: its condition, whether it goes on by
//! itself, the turns judged and the evaluator's newest reason. `store` reads and writes
//! it; `wake` says whether the work it waits on is done.

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

mod store;
mod wake;

use super::{State, timestamp_ms};

pub use dusk_protocol::thread::{Goal, GoalCheck, GoalEvaluation, GoalStatus, GoalVerdict};

fn decode(row: SqliteRow) -> Result<Goal> {
    let turns: i64 = row.try_get("rounds")?;
    let tokens: i64 = row.try_get("spent")?;
    Ok(Goal {
        thread_id: row.try_get("thread_id")?,
        text: row.try_get("text")?,
        status: GoalStatus::parse(row.try_get("status")?)?,
        started_at: row.try_get("started_at")?,
        turns: u32::try_from(turns).context("goal turns out of range")?,
        tokens: u64::try_from(tokens).context("goal tokens out of range")?,
        reason: row.try_get("reason")?,
        note: row.try_get("note")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_state;

    #[tokio::test]
    async fn a_new_goal_replaces_the_old_one_and_starts_active() {
        let state = temp_state("goal-set").await;
        state.create_thread("thread", "test").await.unwrap();
        let first = state.set_goal("thread", "do work").await.unwrap();
        assert_eq!(first.status, GoalStatus::Active);
        state.count_goal_turn("thread", "not yet").await.unwrap();
        state
            .set_goal_status("thread", GoalStatus::Paused, Some("rate limited"))
            .await
            .unwrap();
        let second = state.set_goal("thread", "other work").await.unwrap();
        assert_eq!(second.text, "other work");
        assert_eq!(second.status, GoalStatus::Active);
        assert_eq!((second.turns, second.reason, second.note), (0, None, None));
        assert!(state.set_goal("thread", "  ").await.is_err());
        let long = "x".repeat(dusk_protocol::thread::MAX_GOAL_CHARS + 1);
        assert!(state.set_goal("thread", &long).await.is_err());
        assert!(state.clear_goal("thread").await.unwrap());
        assert!(state.goal("thread").await.unwrap().is_none());
        assert!(!state.clear_goal("thread").await.unwrap());
    }

    /// A restart keeps the goal and starts its counts again.
    #[tokio::test]
    async fn a_restart_keeps_the_goal_and_resets_its_counts() {
        let state = temp_state("goal-restore").await;
        state.create_thread("thread", "test").await.unwrap();
        state.set_goal("thread", "do work").await.unwrap();
        state.count_goal_turn("thread", "not yet").await.unwrap();
        sqlx::query("UPDATE goals SET started_at = 1")
            .execute(&state.pool)
            .await
            .unwrap();
        state.restore_goals().await.unwrap();
        let goal = state.goal("thread").await.unwrap().unwrap();
        assert_eq!(goal.text, "do work");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.turns, 0);
        assert!(goal.started_at > 1);
        assert_eq!(goal.reason.as_deref(), Some("not yet"));
    }

    /// Goals saved under the approved-criteria design: an approved one stays, a draft and a
    /// done one go.
    #[tokio::test]
    async fn migration_keeps_approved_goals_only() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for sql in [
            include_str!("../../db/migrations/0001_threads.sql"),
            include_str!("../../db/migrations/0002_messages.sql"),
            include_str!("../../db/migrations/0003_turns.sql"),
            include_str!("../../db/migrations/0005_model_usage.sql"),
            include_str!("../../db/migrations/0012_goals.sql"),
            include_str!("../../db/migrations/0013_background_jobs.sql"),
            include_str!("../../db/migrations/0016_goal_verification.sql"),
        ] {
            sqlx::raw_sql(sql).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(
            "INSERT INTO threads (id, source, created_at) VALUES ('approved', 'test', 0),
                ('draft', 'test', 0), ('done', 'test', 0);
            INSERT INTO goals (thread_id, text, status, rounds, note, updated_at, approved_at)
            VALUES ('approved', 'ship it', 'paused', 4, 'stopped', 7, 1),
                ('draft', 'draft', 'paused', 0, NULL, 0, NULL),
                ('done', 'history', 'done', 1, NULL, 0, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!("../../db/migrations/0046_goal_evaluator.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let state = State {
            pool,
            episodes: Default::default(),
        };
        let goal = state.goal("approved").await.unwrap().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert_eq!((goal.turns, goal.started_at), (0, 7));
        assert!(state.goal("draft").await.unwrap().is_none());
        assert!(state.goal("done").await.unwrap().is_none());
    }
}
