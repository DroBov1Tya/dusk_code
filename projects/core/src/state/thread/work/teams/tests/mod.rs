//! The team's store: the budget rules, what a run used and the check, each end and
//! scope, migration 0037 with its CHECKs. Here, what they share: two chats, tasks, calls.

mod budget;
mod counts;
mod migration;
mod runs;

use std::{borrow::Cow, time::Duration};

use dusk_protocol::thread::{TeamBudget, TeamEnd, TeamStatus, TeamUsed};
use sqlx::{
    ConnectOptions,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use super::{
    BudgetBy, BudgetPart, Spent, TeamAsk, TeamCeilings, TeamCheck, TeamError, TeamRun, used,
};
use crate::{
    state::{Claim, NewTask, PlanStatus, State},
    testing::temp_dir,
};

pub(super) const LEAD: &str = "lead";
pub(super) const START: i64 = 10_000_000;
pub(super) const MINUTE: i64 = 60_000;

pub(super) fn budget(at_once: u32, steps: u32, tokens: u64, minutes: u32) -> TeamBudget {
    TeamBudget::new(at_once, steps, tokens, minutes).unwrap()
}

pub(super) fn plain() -> TeamBudget {
    budget(3, 400, 4_000_000, 120)
}

pub(super) fn ceilings() -> TeamCeilings {
    TeamCeilings::new(budget(4, 500, 5_000_000, 180))
}

/// A chat `lead` with members `m0`, `m1` and `sub` under `m0`; another chat `other` with
/// `other-m`.
pub(super) async fn chats() -> State {
    let state = State::in_memory().await.unwrap();
    state.create_thread(LEAD, "test").await.unwrap();
    for (id, parent) in [("m0", LEAD), ("m1", LEAD), ("sub", "m0")] {
        state
            .create_agent_thread(id, parent, "general", id)
            .await
            .unwrap();
    }
    state.create_thread("other", "test").await.unwrap();
    state
        .create_agent_thread("other-m", "other", "general", "x")
        .await
        .unwrap();
    state
}

pub(super) async fn add(state: &State, count: usize) -> Vec<String> {
    let tasks: Vec<NewTask> = (0..count)
        .map(|index| NewTask {
            description: format!("task {index}"),
            ..NewTask::default()
        })
        .collect();
    let added = state.add_plan_tasks(LEAD, &tasks).await.unwrap();
    added.into_iter().map(|task| task.task_id).collect()
}

pub(super) async fn claim(state: &State, task: &str, owner: &str) {
    let claim = Claim {
        owner,
        at_once: 8,
        model: Some("model-a"),
        now: START,
    };
    state.claim_task(LEAD, task, claim).await.unwrap();
}

pub(super) async fn usage(state: &State, thread: &str, purpose: &str, tokens: (i64, i64), at: i64) {
    sqlx::query(
        "INSERT INTO model_usage (thread_id, purpose, model, through_sequence, prompt_tokens,
             cached_prompt_tokens, completion_tokens, reasoning_tokens, created_at)
         VALUES (?, ?, 'model-a', 1, ?, 0, ?, 0, ?)",
    )
    .bind(thread)
    .bind(purpose)
    .bind(tokens.0)
    .bind(tokens.1)
    .bind(at)
    .execute(&state.pool)
    .await
    .unwrap();
}
