//! The claim and the lease: one winner, files held, the limit at once, renewal by the
//! owner alone, leases that ran out, and the counters' ends.

use std::sync::Arc;

use super::kit::{LEAD, NOW, add, claim, drive, get, status, task, team, turn, with_files};
use crate::state::{
    Claim, LEASE_MS, MAX_ATTEMPTS, MAX_REWORKS, PassedBy, PlanError, PlanStatus, Unclaimable,
};

fn unclaimable(error: &PlanError) -> &Unclaimable {
    match error {
        PlanError::NotClaimable { why, .. } => why,
        other => panic!("{other}"),
    }
}

#[tokio::test]
async fn of_two_claims_of_one_task_one_wins() {
    let state = team().await;
    let id = add(&state, &[task("one")]).await.remove(0);
    let won = state.claim_task(LEAD, &id, claim("m0")).await.unwrap();
    assert_eq!(won.owner_thread_id.as_deref(), Some("m0"));
    assert_eq!((won.attempts, won.lease_until), (1, Some(NOW + LEASE_MS)));
    let lost = state.claim_task(LEAD, &id, claim("m1")).await.unwrap_err();
    assert!(
        matches!(
            lost,
            PlanError::Illegal {
                from: PlanStatus::Claimed,
                to: PlanStatus::Claimed,
                ..
            }
        ),
        "{lost}"
    );
    assert_eq!(get(&state, &id).await, won);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claims_that_race_never_both_win() {
    let state = Arc::new(team().await);
    for round in 0..20 {
        // One task for eight members, then two tasks that hold one file.
        let one = add(&state, &[task(&format!("one {round}"))])
            .await
            .remove(0);
        let mut racers = Vec::new();
        for member in 0..8 {
            let (state, one) = (state.clone(), one.clone());
            racers.push(tokio::spawn(async move {
                let owner = format!("m{member}");
                state.claim_task(LEAD, &one, claim(&owner)).await.is_ok()
            }));
        }
        let mut winners = 0;
        for racer in racers {
            winners += usize::from(racer.await.unwrap());
        }
        assert_eq!(winners, 1);
        state.return_work(LEAD).await.unwrap();

        let pair = add(
            &state,
            &[
                with_files(&format!("left {round}"), &["src/lib.rs"]),
                with_files(&format!("right {round}"), &["src/"]),
            ],
        )
        .await;
        let mut racers = Vec::new();
        for (member, id) in pair.iter().cloned().enumerate() {
            let state = state.clone();
            racers.push(tokio::spawn(async move {
                let owner = format!("m{member}");
                state.claim_task(LEAD, &id, claim(&owner)).await.is_ok()
            }));
        }
        let mut winners = 0;
        for racer in racers {
            winners += usize::from(racer.await.unwrap());
        }
        assert_eq!(winners, 1);
        state.return_work(LEAD).await.unwrap();
        for id in pair.iter().chain([&one]) {
            state.fail_task(LEAD, id, "round over").await.unwrap();
        }
    }
}

#[tokio::test]
async fn a_task_is_not_given_out_while_another_holds_its_files() {
    let state = team().await;
    let ids = add(
        &state,
        &[
            with_files("holder", &["src/a/", "docs/x.md"]),
            with_files("same file", &["docs/x.md"]),
            with_files("inside", &["src/a/deep/b.rs"]),
            with_files("above", &["src/"]),
            with_files("beside", &["src/ab", "src/ab/", "docs/x.md.bak"]),
            task("nothing"),
        ],
    )
    .await;
    let holder = &ids[0];
    // While the holder is at work, in review, or blocked half way.
    for held_in in [
        PlanStatus::Claimed,
        PlanStatus::Running,
        PlanStatus::Blocked,
        PlanStatus::Review,
    ] {
        match held_in {
            PlanStatus::Claimed => drive(&state, holder, "m0", PlanStatus::Claimed).await,
            PlanStatus::Running => {
                let turn_id = turn(&state, "m0").await;
                state.start_task("m0", holder, &turn_id).await.unwrap();
            }
            PlanStatus::Blocked => {
                state.block_task("m0", holder, None, "stuck").await.unwrap();
            }
            _ => {
                state.unblock_task(LEAD, holder, NOW).await.unwrap();
                state.hand_in("m0", holder).await.unwrap();
            }
        }
        for (at, id) in ids.iter().enumerate().skip(1).take(3) {
            let before = get(&state, id).await;
            let refused = state.claim_task(LEAD, id, claim("m1")).await.unwrap_err();
            assert!(
                matches!(unclaimable(&refused), Unclaimable::Files { holder: by, .. } if by == holder),
                "{held_in:?} {at}: {refused}"
            );
            assert_eq!(get(&state, id).await, before);
        }
    }
    // What does not overlap starts beside it; a task without files always may.
    state.claim_task(LEAD, &ids[4], claim("m1")).await.unwrap();
    state.claim_task(LEAD, &ids[5], claim("m2")).await.unwrap();
    // Done lets go, and so does a task that went back to open.
    state.pass_task(LEAD, holder, PassedBy::Lead).await.unwrap();
    state.release_task(LEAD, &ids[4], "stopped").await.unwrap();
    state.claim_task(LEAD, &ids[3], claim("m3")).await.unwrap();
    let nested = state
        .claim_task(LEAD, &ids[2], claim("m4"))
        .await
        .unwrap_err();
    assert!(
        matches!(unclaimable(&nested), Unclaimable::Files { .. }),
        "{nested}"
    );
    // A failed holder lets go too.
    state.fail_task(LEAD, &ids[3], "no").await.unwrap();
    state.claim_task(LEAD, &ids[2], claim("m4")).await.unwrap();
}

#[tokio::test]
async fn no_more_are_at_work_than_the_team_runs_at_once() {
    let state = team().await;
    let ids = add(&state, &[task("a"), task("b"), task("c"), task("d")]).await;
    let two = |owner| Claim {
        at_once: 2,
        ..claim(owner)
    };
    state.claim_task(LEAD, &ids[0], two("m0")).await.unwrap();
    drive(&state, &ids[1], "m1", PlanStatus::Running).await;
    let full = state
        .claim_task(LEAD, &ids[2], two("m2"))
        .await
        .unwrap_err();
    assert_eq!(*unclaimable(&full), Unclaimable::AtOnce(2));
    assert_eq!(status(&state, &ids[2]).await, PlanStatus::Open);
    let none = state
        .claim_task(
            LEAD,
            &ids[2],
            Claim {
                at_once: 0,
                ..claim("m2")
            },
        )
        .await
        .unwrap_err();
    assert_eq!(*unclaimable(&none), Unclaimable::AtOnce(0));
    // A task in review is not at work.
    state.hand_in("m1", &ids[1]).await.unwrap();
    state.claim_task(LEAD, &ids[2], two("m2")).await.unwrap();

    // A member has one task: not a second while the first is unfinished in any status.
    let busy = state
        .claim_task(LEAD, &ids[3], claim("m1"))
        .await
        .unwrap_err();
    assert_eq!(*unclaimable(&busy), Unclaimable::OwnerBusy);
    state
        .pass_task(LEAD, &ids[1], PassedBy::Lead)
        .await
        .unwrap();
    state.claim_task(LEAD, &ids[3], claim("m1")).await.unwrap();
}

#[tokio::test]
async fn only_the_owner_moves_the_lease_on() {
    let state = team().await;
    let ids = add(&state, &[task("a"), task("b")]).await;
    let id = &ids[0];
    drive(&state, id, "m0", PlanStatus::Claimed).await;
    drive(&state, &ids[1], "m1", PlanStatus::Running).await;
    for thread in ["m1", LEAD, "other", "other-m", "nobody"] {
        assert!(
            state.renew_lease(thread, id, NOW + 50).await.is_err(),
            "{thread}"
        );
    }
    assert_eq!(get(&state, id).await.lease_until, Some(NOW + LEASE_MS));
    // The whole id only: the statement is the design's, with no search for an end.
    state.renew_lease("m0", id, NOW + 50).await.unwrap();
    assert_eq!(get(&state, id).await.lease_until, Some(NOW + 50 + LEASE_MS));
    assert!(
        state
            .renew_lease("m0", &id[id.len() - 8..], NOW + 60)
            .await
            .is_err()
    );
    // No lease to move once the task is out of work.
    let turn_id = turn(&state, "m0").await;
    state.start_task("m0", id, &turn_id).await.unwrap();
    state.renew_lease("m0", id, NOW + 70).await.unwrap();
    state.hand_in("m0", id).await.unwrap();
    let late = state.renew_lease("m0", id, NOW + 80).await.unwrap_err();
    assert!(
        matches!(
            late,
            PlanError::Illegal {
                from: PlanStatus::Review,
                ..
            }
        ),
        "{late}"
    );
    assert_eq!(get(&state, id).await.lease_until, None);
}

#[tokio::test]
async fn a_started_task_marks_the_owners_turn() {
    let state = team().await;
    let id = add(&state, &[task("a")]).await.remove(0);
    state.claim_task(LEAD, &id, claim("m0")).await.unwrap();
    // Another thread's turn is not the owner's: nothing starts.
    let foreign = turn(&state, "m1").await;
    let refused = state.start_task("m0", &id, &foreign).await.unwrap_err();
    assert!(matches!(refused, PlanError::NoSuchTurn(_)), "{refused}");
    assert_eq!(status(&state, &id).await, PlanStatus::Claimed);
    let own = turn(&state, "m0").await;
    state.start_task("m0", &id, &own).await.unwrap();
    let marked: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT turn_id, task_id FROM turns ORDER BY thread_id")
            .fetch_all(&state.pool)
            .await
            .unwrap();
    assert_eq!(marked, [(own, Some(id)), (foreign, None)]);
}

#[tokio::test]
async fn leases_that_ran_out_are_released_and_the_third_attempt_fails() {
    let state = team().await;
    let ids = add(&state, &[task("slow"), task("alive"), task("claimed only")]).await;
    let slow = &ids[0];
    drive(&state, &ids[1], "m1", PlanStatus::Running).await;
    state
        .renew_lease("m1", &ids[1], NOW + LEASE_MS)
        .await
        .unwrap();
    for attempt in 1..=MAX_ATTEMPTS {
        drive(&state, slow, "m0", PlanStatus::Running).await;
        if attempt == 1 {
            drive(&state, &ids[2], "m2", PlanStatus::Claimed).await;
        }
        // Not before its time: a lease ends after `lease_until`, not at it.
        assert!(
            state
                .release_expired(LEAD, NOW + LEASE_MS)
                .await
                .unwrap()
                .is_empty()
        );
        let released = state
            .release_expired(LEAD, NOW + LEASE_MS + 1)
            .await
            .unwrap();
        let slow_now = released.iter().find(|task| task.task_id == *slow).unwrap();
        assert_eq!(released.len(), if attempt == 1 { 2 } else { 1 });
        assert_eq!(slow_now.attempts, attempt);
        assert_eq!(slow_now.lease_until, None);
        assert_eq!(slow_now.note, "the lease ran out");
        if attempt < MAX_ATTEMPTS {
            assert_eq!(slow_now.status, PlanStatus::Open);
            assert_eq!(slow_now.owner_thread_id, None);
        } else {
            // Failed keeps who had it last.
            assert_eq!(slow_now.status, PlanStatus::Failed);
            assert_eq!(slow_now.owner_thread_id.as_deref(), Some("m0"));
        }
    }
    // The renewed one lives; the one only claimed went back once.
    assert_eq!(status(&state, &ids[1]).await, PlanStatus::Running);
    let once = get(&state, &ids[2]).await;
    assert_eq!((once.status, once.attempts), (PlanStatus::Open, 1));
    assert!(
        state
            .release_expired(LEAD, NOW + LEASE_MS + 1)
            .await
            .unwrap()
            .is_empty()
    );

    // Reopened, the counters start again.
    let again = state.reopen_task(LEAD, slow).await.unwrap();
    assert_eq!(
        (again.status, again.attempts, again.reworks),
        (PlanStatus::Open, 0, 0)
    );
    assert_eq!(again.owner_thread_id, None);
    assert_eq!(
        state
            .claim_task(LEAD, slow, claim("m0"))
            .await
            .unwrap()
            .attempts,
        1
    );
}

#[tokio::test]
async fn the_team_s_end_returns_work_without_failing_it() {
    let state = team().await;
    let ids = add(&state, &[task("a"), task("b"), task("c"), task("d")]).await;
    // `a` is on its last attempt.
    for _ in 1..MAX_ATTEMPTS {
        state.claim_task(LEAD, &ids[0], claim("m0")).await.unwrap();
        state.release_task(LEAD, &ids[0], "stopped").await.unwrap();
    }
    drive(&state, &ids[0], "m0", PlanStatus::Running).await;
    drive(&state, &ids[1], "m1", PlanStatus::Claimed).await;
    drive(&state, &ids[2], "m2", PlanStatus::Review).await;
    drive(&state, &ids[3], "m3", PlanStatus::Blocked).await;
    let kept = (get(&state, &ids[2]).await, get(&state, &ids[3]).await);

    let mut returned = state.return_work(LEAD).await.unwrap();
    returned.sort_by(|one, other| one.task_id.cmp(&other.task_id));
    assert_eq!(returned.len(), 2);
    for (task, attempts) in returned.iter().zip([MAX_ATTEMPTS, 1]) {
        assert_eq!(task.status, PlanStatus::Open);
        assert_eq!(
            (task.owner_thread_id.as_deref(), task.lease_until),
            (None, None)
        );
        assert_eq!(task.attempts, attempts, "attempts are left as they were");
    }
    assert_eq!(
        (get(&state, &ids[2]).await, get(&state, &ids[3]).await),
        kept
    );
    assert!(state.return_work(LEAD).await.unwrap().is_empty());
    // A later run gives `a` out again; if that turn is lost too, it fails.
    state.claim_task(LEAD, &ids[0], claim("m0")).await.unwrap();
    let lost = state.release_task(LEAD, &ids[0], "stopped").await.unwrap();
    assert_eq!(lost.status, PlanStatus::Failed);
}

#[tokio::test]
async fn a_task_comes_back_from_review_twice_and_then_fails() {
    let state = team().await;
    let id = add(&state, &[task("a")]).await.remove(0);
    drive(&state, &id, "m0", PlanStatus::Review).await;
    for rework in 1..=MAX_REWORKS {
        let back = state.rework_task(LEAD, &id, "redo", NOW + 9).await.unwrap();
        assert_eq!((back.status, back.reworks), (PlanStatus::Running, rework));
        // The same thread goes on, on a lease that started again.
        assert_eq!(back.owner_thread_id.as_deref(), Some("m0"));
        assert_eq!(back.lease_until, Some(NOW + 9 + LEASE_MS));
        assert_eq!(back.note, "redo");
        state.hand_in("m0", &id).await.unwrap();
    }
    let failed = state
        .rework_task(LEAD, &id, "still wrong", NOW)
        .await
        .unwrap();
    assert_eq!(
        (failed.status, failed.reworks),
        (PlanStatus::Failed, MAX_REWORKS)
    );
    assert_eq!(
        (failed.lease_until, failed.note.as_str()),
        (None, "still wrong")
    );
    assert_eq!(failed.owner_thread_id.as_deref(), Some("m0"));
    let again = state.reopen_task(LEAD, &id).await.unwrap();
    assert_eq!((again.attempts, again.reworks), (0, 0));
}
