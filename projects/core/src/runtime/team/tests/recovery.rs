//! A stop of the core between two steps of giving a task out, then a dispatcher that
//! knows nothing but the database.

use std::sync::Arc;

use super::*;
use crate::{
    runtime::thread::start::start_member_thread,
    state::{Claim, timestamp_ms},
};

/// A running team with one task, and a member thread made for it, as a stop right
/// after the thread was made leaves them.
async fn stopped_after_the_thread(rig: &Rig) -> (String, String) {
    let ids = rig.plan(&[task("one")]).await;
    let now = timestamp_ms().unwrap();
    let limits = rig.launcher().config.current().limits();
    let ceilings = TeamCeilings::from_limits(&limits).unwrap();
    rig.state()
        .start_team(&rig.lead, ceilings.budget(), ceilings, now)
        .await
        .unwrap();
    let paths = &rig.launcher().paths;
    let made = start_member_thread(rig.state(), paths, &rig.lead, "general", None)
        .await
        .unwrap();
    (ids[0].clone(), made.thread_id)
}

/// A launcher as a new process has it: nothing of the team in memory.
fn restarted(rig: &Rig) -> Launcher {
    Launcher {
        team: Arc::default(),
        ..rig.launcher().clone()
    }
}

#[tokio::test]
async fn a_stop_between_the_thread_and_the_claim_leaves_a_thread_the_next_pass_takes() {
    let rig = Rig::new("team_kill_thread", all_done).await;
    let (id, made) = stopped_after_the_thread(&rig).await;

    let now = timestamp_ms().unwrap();
    let given = restarted(&rig).team_pass_at(&rig.lead, now).await.unwrap();

    assert_eq!(given, 1);
    assert_eq!(rig.members().await, [made.as_str()], "no second member");
    until("the member handed in", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;
    assert_eq!(rig.task(&id).await.owner_thread_id, Some(made.clone()));
    assert_eq!(rig.uids(&made).await, [format!("card:{id}:1")]);
}

#[tokio::test]
async fn a_stop_between_the_claim_and_the_card_delivers_the_card_once() {
    let rig = Rig::new("team_kill_claim", all_done).await;
    let (id, made) = stopped_after_the_thread(&rig).await;
    let now = timestamp_ms().unwrap();
    let claim = Claim {
        owner: &made,
        at_once: 3,
        model: None,
        now,
    };
    rig.state().claim_task(&rig.lead, &id, claim).await.unwrap();
    assert!(rig.uids(&made).await.is_empty());

    let fresh = restarted(&rig);
    assert_eq!(fresh.team_pass_at(&rig.lead, now).await.unwrap(), 0);
    fresh.team_pass_at(&rig.lead, now).await.unwrap();

    until("the member handed in", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;
    assert_eq!(rig.members().await, [made.as_str()], "no second member");
    assert_eq!(rig.uids(&made).await, [format!("card:{id}:1")]);
    assert_eq!(rig.turns(&made).await, (1, false), "the task ran once");
    assert_eq!(rig.task(&id).await.attempts, 1);
}

#[tokio::test]
async fn a_claimed_task_nobody_works_on_reopens_when_its_lease_runs_out() {
    let rig = Rig::new("team_kill_lease", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Held(DONE.into()),
    })
    .await;
    let (id, made) = stopped_after_the_thread(&rig).await;
    let now = timestamp_ms().unwrap();
    let claim = Claim {
        owner: &made,
        at_once: 3,
        model: None,
        now: now - crate::state::LEASE_MS - 1_000,
    };
    rig.state().claim_task(&rig.lead, &id, claim).await.unwrap();

    // The lease is over before any pass could deliver the card: the task is given out
    // again, and the thread that never read anything is the one that takes it.
    assert_eq!(
        restarted(&rig).team_pass_at(&rig.lead, now).await.unwrap(),
        1
    );

    let task = rig.task(&id).await;
    assert_eq!(
        (task.attempts, task.owner_thread_id),
        (2, Some(made.clone()))
    );
    assert_eq!(rig.members().await, [made.as_str()]);
    assert_eq!(rig.uids(&made).await, [format!("card:{id}:2")]);
}
