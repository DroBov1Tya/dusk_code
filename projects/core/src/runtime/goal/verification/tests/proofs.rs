//! What counts as proof: the contract, semantic confirmation, failed results and forged ids.

use super::*;

#[tokio::test]
async fn only_runtime_can_complete_an_approved_all_pass_contract() {
    let (state, goal) = approved("goal-pass", command()).await;
    assert!(
        state
            .set_goal_status("thread", GoalStatus::Done, Some("model says done"))
            .await
            .is_err()
    );
    let id = proof(&state).await;
    request(&state, &goal, &id).await;
    assert_eq!(
        state.goal("thread").await.unwrap().unwrap().status,
        GoalStatus::Active
    );
    let reviewed = verify(&state, "thread").await.unwrap().unwrap();
    assert_eq!(reviewed.status, GoalStatus::Done);
    assert_eq!(reviewed.verification, "passed");
    assert!(
        verify(&state, "thread").await.unwrap().is_none(),
        "completion cannot reenter"
    );
}

#[tokio::test]
async fn semantic_criterion_needs_confirmation_of_the_exact_current_result() {
    let (state, goal) = approved("goal-human", GoalCheck::UserConfirmation).await;
    let id = result(
        &state,
        "read_file",
        json!({"path":"artifact"}),
        "succeeded",
        None,
        true,
    )
    .await;
    request(&state, &goal, &id).await;
    let reviewed = verify(&state, "thread").await.unwrap().unwrap();
    assert_eq!(reviewed.status, GoalStatus::Active);
    assert_eq!(reviewed.verification, "needs_user");
    assert!(
        state
            .confirm_goal("thread", "stale", "check", &id)
            .await
            .is_err()
    );
    assert!(
        state
            .confirm_goal("thread", &goal.revision, "check", "tool:invented")
            .await
            .is_err()
    );
    state
        .confirm_goal("thread", &goal.revision, "check", &id)
        .await
        .unwrap();
    assert_eq!(
        verify(&state, "thread").await.unwrap().unwrap().status,
        GoalStatus::Done
    );
}

#[tokio::test]
async fn failed_cancelled_unknown_nonzero_and_interrupted_results_never_pass() {
    for (index, outcome, code, completed) in [
        (0, "failed", Some(3), true),
        (1, "cancelled", None, true),
        (2, "unknown", Some(0), true),
        (3, "succeeded", Some(3), true),
        (4, "succeeded", Some(0), false),
        (5, "failed", None, true),
    ] {
        let (state, goal) = approved(&format!("goal-bad-{index}"), command()).await;
        let id = result(
            &state,
            "run_command",
            json!({"command":"cargo test --locked"}),
            outcome,
            code,
            completed,
        )
        .await;
        request(&state, &goal, &id).await;
        let reviewed = verify(&state, "thread").await.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Active, "{index}");
        assert_eq!(reviewed.verification, "gaps");
    }
}

#[tokio::test]
async fn fabricated_foreign_wrong_command_and_old_revision_ids_do_not_pass() {
    let (state, goal) = approved("goal-identities", command()).await;
    let old = proof(&state).await;
    let draft = state.set_goal("thread", "new goal").await.unwrap();
    let draft = state
        .propose_goal("thread", &draft.revision, vec![criterion(command())])
        .await
        .unwrap();
    let current = state.approve_goal("thread", &draft.revision).await.unwrap();
    assert_ne!(current.revision, goal.revision);
    let wrong = result(
        &state,
        "run_command",
        json!({"command":"echo passed"}),
        "succeeded",
        Some(0),
        true,
    )
    .await;
    for id in ["tool:invented".to_owned(), old, wrong] {
        request(&state, &current, &id).await;
        let reviewed = verify(&state, "thread").await.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Active);
        assert_eq!(reviewed.verification, "gaps");
    }
    state.create_thread("foreign", "test").await.unwrap();
    // Even a valid execution cannot be reattributed to another owning thread.
    let actual_id = proof(&state).await;
    let epoch: i64 = sqlx::query_scalar("SELECT work_epoch FROM goals WHERE thread_id = 'thread'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let mut tx = state.pool.begin().await.unwrap();
    assert!(
        read_evidence(
            &mut tx,
            "thread",
            &current.revision,
            0,
            0,
            epoch,
            &actual_id
        )
        .await
        .unwrap()
        .is_some()
    );
    assert!(
        read_evidence(
            &mut tx,
            "foreign",
            &current.revision,
            0,
            0,
            epoch,
            &actual_id
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn later_changes_invalidate_proofs_within_the_same_revision() {
    let (state, goal) = approved("goal-stale-work", command()).await;
    let old = proof(&state).await;
    result(
        &state,
        "files__write_file",
        json!({"path":"src/lib.rs"}),
        "succeeded",
        None,
        true,
    )
    .await;
    request(&state, &goal, &old).await;
    assert_eq!(
        verify(&state, "thread")
            .await
            .unwrap()
            .unwrap()
            .verification,
        "gaps"
    );
    let fresh = proof(&state).await;
    request(&state, &goal, &fresh).await;
    assert_eq!(
        verify(&state, "thread").await.unwrap().unwrap().status,
        GoalStatus::Done
    );
}
