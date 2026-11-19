//! A stop of the core between two steps of giving a task out, then a dispatcher that
//! knows nothing but the database.

use std::sync::Arc;

use super::*;
use crate::{
    runtime::thread::start::start_member_thread,
    state::{Claim, TurnAnswer, TurnOutcome, timestamp_ms},
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

/// A member's task as a stop of the core leaves it right after its turn ended: the
/// card read, the turn over, and nothing of the turn's end done to the task.
async fn stopped_after_the_turn(rig: &Rig, outcome: TurnOutcome) -> (String, String) {
    let (id, made) = stopped_after_the_thread(rig).await;
    let state = rig.state();
    let claim = Claim {
        owner: &made,
        at_once: 3,
        model: None,
        now: timestamp_ms().unwrap(),
    };
    let claimed = state.claim_task(&rig.lead, &id, claim).await.unwrap();
    let card = super::super::card::uid(&claimed);
    state
        .add_message_from(&made, &card, "the card", Some(&rig.lead))
        .await
        .unwrap();
    let turn = state.start_turn(&made).await.unwrap().unwrap();
    state.start_task(&made, &id, &turn.turn_id).await.unwrap();
    state.finish_turn(&turn.turn_id, outcome).await.unwrap();
    (id, made)
}

#[tokio::test]
async fn a_task_whose_turn_a_stop_cut_off_returns_to_the_pool_exactly_once() {
    let rig = Rig::new("team_settle_lost", hang).await;
    let (id, dead) = stopped_after_the_turn(&rig, TurnOutcome::Interrupted(None)).await;
    let fresh = restarted(&rig);

    fresh.team_settle(&rig.lead).await;

    // Released and given out again in the same breath, to a thread that has read
    // nothing; the dead member's thread is not started again.
    let task = rig.task(&id).await;
    assert_eq!(task.attempts, 2);
    assert!(task.owner_thread_id.is_some() && task.owner_thread_id != Some(dead.clone()));
    assert_eq!(rig.turns(&dead).await, (1, false));

    // Settling again finds nothing left by a stopped core: the task is not released
    // a second time.
    fresh.team_settle(&rig.lead).await;
    fresh.team_settle(&rig.lead).await;
    let again = rig.task(&id).await;
    assert_eq!(
        (again.attempts, again.owner_thread_id),
        (2, task.owner_thread_id)
    );
    assert_eq!(rig.members().await.len(), 2);
}

#[tokio::test]
async fn a_turn_that_completed_before_the_stop_is_handed_in_and_not_done_twice() {
    let rig = Rig::new("team_settle_done", hang).await;
    let answer = TurnAnswer {
        content: DONE.into(),
        reasoning: None,
    };
    let (id, made) = stopped_after_the_turn(&rig, TurnOutcome::Completed(answer)).await;
    let fresh = restarted(&rig);

    fresh.team_settle(&rig.lead).await;
    fresh.team_settle(&rig.lead).await;

    let task = rig.task(&id).await;
    assert_eq!((task.status, task.attempts), (PlanStatus::Review, 1));
    assert_eq!(task.owner_thread_id, Some(made.clone()));
    // The work was not given out again, and the lead reads of it once.
    assert_eq!(rig.members().await, [made.as_str()]);
    assert_eq!(rig.turns(&made).await, (1, false));
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("waits for you: no gate. Result: it works now."));
}

#[tokio::test]
async fn an_end_cut_off_before_its_work_was_returned_is_finished() {
    let rig = Rig::new("team_settle_end", hang).await;
    let (id, made) = stopped_after_the_thread(&rig).await;
    let now = timestamp_ms().unwrap();
    let claim = Claim {
        owner: &made,
        at_once: 3,
        model: None,
        now,
    };
    rig.state().claim_task(&rig.lead, &id, claim).await.unwrap();
    // The end is stored first; the stop came before the tasks at work were returned.
    sqlx::query("UPDATE teams SET status = 'ended', ended_why = 'stopped', ended_at = ?")
        .bind(now + 1)
        .execute(&rig.state().pool)
        .await
        .unwrap();

    restarted(&rig).team_settle(&rig.lead).await;

    let task = rig.task(&id).await;
    assert_eq!(
        (task.status, task.owner_thread_id),
        (PlanStatus::Open, None)
    );
    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    assert!(rig.uids(&made).await.is_empty(), "nothing was given out");
}
