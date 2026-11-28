//! The state machine, pair by pair: every move from every status, by its owner and by
//! another thread. A legal one changes exactly the columns it says; any other is refused
//! with a typed error and changes nothing in the plan.

use super::kit::{LEAD, NOW, add, claim, drive, get, keyed, task, team, turn};
use crate::state::{
    BlockedOn, LEASE_MS, PassedBy, PlanError, PlanStatus, PlanTask, State, TaskEdit, WaitsOn,
};

/// Stands for the time of the move in `retried_at`, which the test cannot know before.
const RETRIED_NOW: i64 = i64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum From {
    Open,
    Claimed,
    Running,
    /// The member said it cannot go on.
    BlockedMember,
    /// A task it depends on failed.
    BlockedBehind,
    Review,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Claim,
    Start {
        owner: bool,
    },
    Renew {
        owner: bool,
    },
    Release,
    HandIn {
        owner: bool,
    },
    Block {
        owner: bool,
    },
    Unblock,
    Hold,
    GateJob,
    Pass(PassedBy),
    Rework,
    Fail,
    Reopen,
    Edit,
    /// The old board's `complete`, as the tool calls it today.
    BoardComplete,
}

const FROMS: [From; 8] = [
    From::Open,
    From::Claimed,
    From::Running,
    From::BlockedMember,
    From::BlockedBehind,
    From::Review,
    From::Done,
    From::Failed,
];

const MOVES: [Move; 21] = [
    Move::Claim,
    Move::Start { owner: true },
    Move::Start { owner: false },
    Move::Renew { owner: true },
    Move::Renew { owner: false },
    Move::Release,
    Move::HandIn { owner: true },
    Move::HandIn { owner: false },
    Move::Block { owner: true },
    Move::Block { owner: false },
    Move::Unblock,
    Move::Hold,
    Move::GateJob,
    Move::Pass(PassedBy::Gate),
    Move::Pass(PassedBy::Lead),
    Move::Pass(PassedBy::User),
    Move::Rework,
    Move::Fail,
    Move::Reopen,
    Move::Edit,
    Move::BoardComplete,
];

/// The table of section 4 for a task without a gate: what the row becomes, or `false`
/// when the pair is not in the table.
fn legal(from: From, mv: Move, task: &mut PlanTask) -> bool {
    use PlanStatus::{Blocked, Claimed, Done, Failed, Open, Review, Running};
    let lease = Some(NOW + LEASE_MS);
    match (from, mv) {
        (From::Open, Move::Claim) => {
            task.status = Claimed;
            task.owner_thread_id = Some("m0".to_owned());
            task.lease_until = lease;
            task.attempts += 1;
            task.model = Some("model-a".to_owned());
        }
        (From::Claimed, Move::Start { owner: true }) => task.status = Running,
        (From::Claimed | From::Running, Move::Renew { owner: true }) => {
            task.lease_until = Some(NOW + 5 + LEASE_MS);
        }
        (From::Claimed | From::Running, Move::Release) => {
            task.status = Open;
            // The member that lost it is kept: the next claim may give it back.
            task.last_owner_thread_id = task.owner_thread_id.take();
            task.lease_until = None;
            task.note = "stopped".to_owned();
        }
        (From::Running, Move::HandIn { owner: true }) => {
            task.status = Review;
            task.lease_until = None;
        }
        (From::Running, Move::Block { owner: true }) => {
            task.status = Blocked;
            task.lease_until = None;
            task.waits_on = Some(WaitsOn::Message(9));
            task.note = "need y".to_owned();
        }
        (From::BlockedMember, Move::Unblock) => {
            task.status = Running;
            task.lease_until = lease;
            task.waits_on = None;
        }
        (From::Review, Move::Hold) => {
            task.waits_on = Some(WaitsOn::Lead);
            task.note = "outside its files: x".to_owned();
        }
        (From::Review, Move::GateJob) => task.gate_job = Some("job-1".to_owned()),
        (From::Review, Move::Pass(PassedBy::Lead)) => task.status = Done,
        (From::Review, Move::Rework) => {
            task.status = Running;
            task.lease_until = lease;
            task.reworks += 1;
            task.note = "redo".to_owned();
        }
        (From::Done | From::Failed, Move::Fail) => return false,
        (_, Move::Fail) => {
            task.status = Failed;
            task.lease_until = None;
            task.waits_on = None;
            task.note = "gave up".to_owned();
        }
        (From::Failed | From::BlockedMember, Move::Reopen) => {
            if from == From::Failed {
                task.attempts = 0;
                task.reworks = 0;
                task.retries += 1;
                task.retried_at = Some(RETRIED_NOW);
            }
            task.status = Open;
            task.owner_thread_id = None;
            task.last_owner_thread_id = None;
            task.waits_on = None;
            task.note = String::new();
        }
        (From::Open | From::BlockedBehind | From::Failed, Move::Edit) => {
            task.description = "new words".to_owned();
            task.last_owner_thread_id = None;
        }
        (From::Open, Move::BoardComplete) => task.status = Done,
        (From::Done, Move::BoardComplete) => {}
        _ => return false,
    }
    true
}

async fn make(state: &State, mv: Move) -> Result<(), PlanError> {
    let target = "target";
    // The thread that is not the owner is a member of the same plan, so only the
    // owner check stands between it and the task.
    let who = |owner| if owner { "m0" } else { "m1" };
    let id = add_or_find(state, target).await;
    match mv {
        Move::Claim => state.claim_task(LEAD, &id, claim("m0")).await.map(drop),
        Move::Start { owner } => {
            let turn_id = turn(state, who(owner)).await;
            state.start_task(who(owner), &id, &turn_id).await.map(drop)
        }
        Move::Renew { owner } => state.renew_lease(who(owner), &id, NOW + 5).await,
        Move::Release => state.release_task(LEAD, &id, "stopped").await.map(drop),
        Move::HandIn { owner } => state.hand_in(who(owner), &id).await.map(drop),
        Move::Block { owner } => state
            .block_task(who(owner), &id, Some(&BlockedOn::Message(9)), "need y")
            .await
            .map(drop),
        Move::Unblock => state.unblock_task(LEAD, &id, NOW).await.map(drop),
        Move::Hold => state
            .hold_for_lead(LEAD, &id, "outside its files: x")
            .await
            .map(drop),
        Move::GateJob => state.set_gate_job(LEAD, &id, "job-1").await.map(drop),
        Move::Pass(by) => state.pass_task(LEAD, &id, by).await.map(drop),
        Move::Rework => state.rework_task(LEAD, &id, "redo", NOW).await.map(drop),
        Move::Fail => state.fail_task(LEAD, &id, "gave up").await.map(drop),
        Move::Reopen => state.reopen_task(LEAD, &id).await.map(drop),
        Move::Edit => {
            let edit = TaskEdit {
                description: Some("new words".to_owned()),
                ..TaskEdit::default()
            };
            state.edit_plan_task(LEAD, &id, &edit).await.map(drop)
        }
        Move::BoardComplete => state
            .complete(LEAD, &id)
            .await
            .map(drop)
            // The board's errors are plain text; the table only asks that it refuses.
            .map_err(PlanError::Store),
    }
}

async fn add_or_find(state: &State, text: &str) -> String {
    let plan = state.plan(LEAD).await.unwrap();
    plan.into_iter()
        .find(|task| task.description == text)
        .map(|task| task.task_id)
        .unwrap()
}

/// A plan with the task `target` in `from`, the turn its member started with, and the
/// task `before` it when the status needs one.
async fn plan_in(from: From) -> State {
    let state = team().await;
    match from {
        From::BlockedBehind => {
            let ids = add(
                &state,
                &[keyed("before", &[]), keyed("target", &["before"])],
            )
            .await;
            state.fail_task(LEAD, &ids[0], "no").await.unwrap();
        }
        _ => {
            let ids = add(&state, &[task("target")]).await;
            let to = match from {
                From::Open => PlanStatus::Open,
                From::Claimed => PlanStatus::Claimed,
                From::Running => PlanStatus::Running,
                From::BlockedMember => PlanStatus::Blocked,
                From::Review => PlanStatus::Review,
                From::Done => PlanStatus::Done,
                From::Failed => PlanStatus::Failed,
                From::BlockedBehind => unreachable!(),
            };
            drive(&state, &ids[0], "m0", to).await;
        }
    }
    state
}

#[tokio::test]
async fn every_move_from_every_status_is_made_or_refused_whole() {
    for from in FROMS {
        for mv in MOVES {
            let state = plan_in(from).await;
            let before = state.plan(LEAD).await.unwrap();
            let target = before
                .iter()
                .find(|task| task.description == "target")
                .unwrap()
                .clone();
            let mut expected = target.clone();
            let is_legal = legal(from, mv, &mut expected);

            let made = make(&state, mv).await;

            let after = state.plan(LEAD).await.unwrap();
            if !is_legal {
                let Err(error) = made else {
                    panic!("{from:?} {mv:?} must be refused");
                };
                let typed = match mv {
                    Move::BoardComplete => true,
                    _ => !matches!(error, PlanError::Store(_)),
                };
                assert!(typed, "{from:?} {mv:?}: refused untyped: {error}");
                assert_eq!(
                    after, before,
                    "{from:?} {mv:?} was refused and changed the plan"
                );
                continue;
            }
            made.unwrap_or_else(|error| panic!("{from:?} {mv:?} is legal: {error}"));
            let mut now = get(&state, &target.task_id).await;
            // A retry is stamped with the time of the move.
            if expected.retried_at == Some(RETRIED_NOW) {
                assert_eq!(now.retried_at, Some(now.updated_at), "{from:?} {mv:?}");
                expected.retried_at = now.retried_at;
            }
            // When it changed is the one column every move may write.
            now.updated_at = expected.updated_at;
            assert_eq!(now, expected, "{from:?} {mv:?} changed other columns");
            // No other task moved (the task `before` of a blocked one stays failed).
            for (was, is) in before.iter().zip(&after) {
                if was.task_id != target.task_id {
                    assert_eq!(was, is, "{from:?} {mv:?} changed another task");
                }
            }
        }
    }
}

#[tokio::test]
async fn a_refusal_names_what_was_wrong() {
    let state = plan_in(From::Running).await;
    let id = add_or_find(&state, "target").await;
    let other = state.hand_in("m1", &id).await.unwrap_err();
    assert!(matches!(other, PlanError::NotOwner { .. }), "{other}");
    let illegal = state
        .pass_task(LEAD, &id, PassedBy::Lead)
        .await
        .unwrap_err();
    assert!(
        matches!(
            illegal,
            PlanError::Illegal {
                from: PlanStatus::Running,
                to: PlanStatus::Done,
                ..
            }
        ),
        "{illegal}"
    );
    let short = &id[id.len() - 8..];
    assert_eq!(
        illegal.to_string(),
        format!("task #{short} is running; it cannot become done")
    );
    let behind = plan_in(From::BlockedBehind).await;
    let id = add_or_find(&behind, "target").await;
    let reopen = behind.reopen_task(LEAD, &id).await.unwrap_err();
    assert!(matches!(reopen, PlanError::BehindFailed { .. }), "{reopen}");
}

#[tokio::test]
async fn a_review_is_passed_only_by_the_one_its_gate_names() {
    use dusk_protocol::thread::GoalCheck;
    let command = Some(GoalCheck::Command {
        command: "cargo test".to_owned(),
    });
    let confirm = Some(GoalCheck::UserConfirmation);
    // (gate, held for the lead, who, passes)
    let cases = [
        (None, false, PassedBy::Lead, true),
        (None, false, PassedBy::Gate, false),
        (None, false, PassedBy::User, false),
        (command.clone(), false, PassedBy::Gate, true),
        (command.clone(), false, PassedBy::Lead, false),
        (command.clone(), false, PassedBy::User, false),
        (command.clone(), true, PassedBy::Gate, false),
        (command.clone(), true, PassedBy::Lead, true),
        (confirm.clone(), false, PassedBy::User, true),
        (confirm.clone(), false, PassedBy::Lead, false),
        (confirm.clone(), false, PassedBy::Gate, false),
        (confirm.clone(), true, PassedBy::Lead, true),
    ];
    for (gate, held, by, passes) in cases {
        let state = team().await;
        let mut new = task("target");
        new.gate = gate.clone();
        let id = add(&state, &[new]).await.remove(0);
        drive(&state, &id, "m0", PlanStatus::Review).await;
        if held {
            state.hold_for_lead(LEAD, &id, "outside").await.unwrap();
        }
        let before = get(&state, &id).await;
        let made = state.pass_task(LEAD, &id, by).await;
        if passes {
            assert_eq!(
                made.unwrap().status,
                PlanStatus::Done,
                "{gate:?} {held} {by:?}"
            );
        } else {
            let error = made.unwrap_err();
            assert!(
                matches!(error, PlanError::Gate { .. }),
                "{gate:?} {by:?}: {error}"
            );
            assert_eq!(get(&state, &id).await, before);
        }
    }
}
