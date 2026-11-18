//! What the plan's tests share: a chat with members, tasks in a wanted status.

use sqlx::Row;

use crate::state::{BlockedOn, Claim, NewTask, PassedBy, PlanStatus, PlanTask, State, TaskFiles};

pub const LEAD: &str = "lead";
pub const NOW: i64 = 1_000_000;

/// A chat `lead` with members `m0` to `m7`, and another chat `other` with `other-m`.
pub async fn team() -> State {
    let state = State::in_memory().await.unwrap();
    state.create_thread(LEAD, "test").await.unwrap();
    for member in 0..8 {
        let id = format!("m{member}");
        state
            .create_agent_thread(&id, LEAD, "general", &id)
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

pub fn task(text: &str) -> NewTask {
    NewTask {
        description: text.to_owned(),
        ..NewTask::default()
    }
}

pub fn keyed(key: &str, after: &[&str]) -> NewTask {
    NewTask {
        key: Some(key.to_owned()),
        after: after.iter().map(|after| (*after).to_owned()).collect(),
        ..task(key)
    }
}

pub fn with_files(text: &str, files: &[&str]) -> NewTask {
    NewTask {
        files: TaskFiles::new(files).unwrap(),
        ..task(text)
    }
}

pub async fn add(state: &State, tasks: &[NewTask]) -> Vec<String> {
    let added = state.add_plan_tasks(LEAD, tasks).await.unwrap();
    added.into_iter().map(|task| task.task_id).collect()
}

pub fn claim(owner: &str) -> Claim<'_> {
    Claim {
        owner,
        at_once: 8,
        model: Some("model-a"),
        now: NOW,
    }
}

/// A finished turn of `thread`, for `start_task` to mark.
pub async fn turn(state: &State, thread: &str) -> String {
    let turn_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at)
         VALUES (?, ?, 1, 'interrupted', 0, 0)",
    )
    .bind(&turn_id)
    .bind(thread)
    .execute(&state.pool)
    .await
    .unwrap();
    turn_id
}

/// Moves an open task of `LEAD`'s plan to `to` by the legal moves, with `owner` as its
/// member. Blocked here is a member's; failed is the lead's `fail` of an open task.
pub async fn drive(state: &State, task: &str, owner: &str, to: PlanStatus) {
    use PlanStatus::{Blocked, Claimed, Done, Failed, Open, Review, Running};
    if to == Open {
        return;
    }
    if to == Failed {
        state.fail_task(LEAD, task, "no").await.unwrap();
        return;
    }
    state.claim_task(LEAD, task, claim(owner)).await.unwrap();
    if to == Claimed {
        return;
    }
    let turn_id = turn(state, owner).await;
    state.start_task(owner, task, &turn_id).await.unwrap();
    match to {
        Running => {}
        Blocked => {
            let waits = BlockedOn::Message(7);
            state
                .block_task(owner, task, Some(&waits), "need x")
                .await
                .unwrap();
        }
        Review | Done => {
            state.hand_in(owner, task).await.unwrap();
            if to == Done {
                state.pass_task(LEAD, task, PassedBy::Lead).await.unwrap();
            }
        }
        Open | Claimed | Failed => unreachable!(),
    }
}

pub async fn get(state: &State, task: &str) -> PlanTask {
    state.plan_task(LEAD, task).await.unwrap()
}

pub async fn status(state: &State, task: &str) -> PlanStatus {
    get(state, task).await.status
}

/// `done` and `status` of every task, as the table has them.
pub async fn done_and_status(state: &State) -> Vec<(i64, String)> {
    sqlx::query("SELECT done, status FROM tasks")
        .fetch_all(&state.pool)
        .await
        .unwrap()
        .iter()
        .map(|row| (row.get("done"), row.get("status")))
        .collect()
}
