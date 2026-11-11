//! Epochs: later changes, pauses and resumes invalidate proofs; jobs keep their own.

use super::*;

async fn epoch(state: &State) -> i64 {
    sqlx::query_scalar("SELECT work_epoch FROM goals WHERE thread_id = 'thread'")
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

async fn deliver_jobs(state: &State) {
    for pending in state.pending_messages("thread").await.unwrap() {
        state.admit_pending("thread", &pending).await.unwrap();
    }
    let delivery = state.start_turn("thread").await.unwrap().unwrap();
    state
        .finish_turn(&delivery.turn_id, TurnOutcome::Completed(answer()))
        .await
        .unwrap();
}

#[tokio::test]
async fn paused_file_and_shell_mutations_invalidate_proofs_before_resume() {
    for (name, arguments) in [
        (
            "files__write_file",
            json!({"path":"src/lib.rs","content":"changed"}),
        ),
        (
            "run_command",
            json!({"command":"printf changed > src/lib.rs"}),
        ),
    ] {
        let (state, goal) = approved(&format!("goal-paused-{name}"), command()).await;
        let old = proof(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Paused, None)
            .await
            .unwrap();
        let before = epoch(&state).await;
        result(&state, name, arguments, "succeeded", Some(0), true).await;
        assert!(
            epoch(&state).await > before,
            "paused mutations must invalidate immediately"
        );
        state
            .set_goal_status("thread", GoalStatus::Active, None)
            .await
            .unwrap();
        request(&state, &goal, &old).await;
        let reviewed = verify(&state, "thread").await.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Active);
        assert_eq!(reviewed.verification, "gaps");
        let fresh = proof(&state).await;
        request(&state, &goal, &fresh).await;
        assert_eq!(
            verify(&state, "thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }
}

#[tokio::test]
async fn resume_requires_fresh_proofs_even_without_a_recorded_mutation() {
    let (state, goal) = approved("goal-resume-fence", command()).await;
    let old = proof(&state).await;
    state
        .set_goal_status("thread", GoalStatus::Paused, None)
        .await
        .unwrap();
    let before = epoch(&state).await;
    state
        .set_goal_status("thread", GoalStatus::Active, None)
        .await
        .unwrap();
    assert!(epoch(&state).await > before);
    request(&state, &goal, &old).await;
    assert_eq!(
        verify(&state, "thread")
            .await
            .unwrap()
            .unwrap()
            .verification,
        "gaps"
    );
}

#[tokio::test]
async fn a_check_job_started_before_resume_cannot_refresh_its_launch_epoch() {
    let (state, goal) = approved("goal-old-job-resume", command()).await;
    state
        .set_goal_status("thread", GoalStatus::Paused, None)
        .await
        .unwrap();
    state
        .create_job(
            "thread",
            "old-job",
            "cargo test --locked",
            "runtime",
            "/lease",
        )
        .await
        .unwrap();
    let before = epoch(&state).await;
    state
        .set_goal_status("thread", GoalStatus::Active, None)
        .await
        .unwrap();
    assert!(epoch(&state).await > before);
    state
        .finish_job("old-job", "runtime", "completed", "exit code: 0")
        .await
        .unwrap();
    deliver_jobs(&state).await;
    let launch: i64 =
        sqlx::query_scalar("SELECT goal_epoch FROM background_jobs WHERE job_id = 'old-job'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        launch, before,
        "completion must retain the original check epoch"
    );
    request(&state, &goal, "job:old-job").await;
    assert_eq!(
        verify(&state, "thread")
            .await
            .unwrap()
            .unwrap()
            .verification,
        "gaps"
    );
    state
        .create_job(
            "thread",
            "fresh-job",
            "cargo test --locked",
            "runtime",
            "/lease",
        )
        .await
        .unwrap();
    state
        .finish_job("fresh-job", "runtime", "completed", "exit code: 0")
        .await
        .unwrap();
    deliver_jobs(&state).await;
    request(&state, &goal, "job:fresh-job").await;
    assert_eq!(
        verify(&state, "thread").await.unwrap().unwrap().status,
        GoalStatus::Done
    );
}

#[tokio::test]
async fn a_background_check_before_a_later_batch_mutation_keeps_its_own_epoch() {
    let (state, goal) = approved("goal-job-batch-order", command()).await;
    let turn = turn(&state).await;
    state
        .record_tool_calls(
            &turn.turn_id,
            answer(),
            vec![
                ToolCall {
                    id: "check-job".into(),
                    name: "run_command".into(),
                    arguments: json!({"command":"cargo test --locked","background":true}),
                },
                ToolCall {
                    id: "edit-after".into(),
                    name: "files__write_file".into(),
                    arguments: json!({"path":"src/lib.rs"}),
                },
            ],
        )
        .await
        .unwrap();
    assert!(
        state
            .create_job_for_call(
                "thread",
                "forged-job",
                "cargo test --locked",
                "runtime",
                "/lease",
                Some("invented")
            )
            .await
            .is_err()
    );
    state
        .create_job_for_call(
            "thread",
            "job",
            "cargo test --locked",
            "runtime",
            "/lease",
            Some("check-job"),
        )
        .await
        .unwrap();
    state
        .record_tool_result_with_outcome(
            &turn.turn_id,
            "check-job",
            "job started".into(),
            Vec::new(),
            "succeeded",
            None,
        )
        .await
        .unwrap();
    state
        .record_tool_result_with_outcome(
            &turn.turn_id,
            "edit-after",
            "changed".into(),
            Vec::new(),
            "succeeded",
            None,
        )
        .await
        .unwrap();
    state
        .finish_turn(&turn.turn_id, TurnOutcome::Completed(answer()))
        .await
        .unwrap();
    state
        .finish_job("job", "runtime", "completed", "exit code: 0")
        .await
        .unwrap();
    deliver_jobs(&state).await;
    let launch: i64 =
        sqlx::query_scalar("SELECT goal_epoch FROM background_jobs WHERE job_id = 'job'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(launch < epoch(&state).await);
    request(&state, &goal, "job:job").await;
    assert_eq!(
        verify(&state, "thread")
            .await
            .unwrap()
            .unwrap()
            .verification,
        "gaps"
    );
}

#[tokio::test]
async fn changing_jobs_invalidate_proofs_while_the_contract_is_paused() {
    let (state, goal) = approved("goal-paused-mutating-job", command()).await;
    let old = proof(&state).await;
    state
        .set_goal_status("thread", GoalStatus::Paused, None)
        .await
        .unwrap();
    let before = epoch(&state).await;
    state
        .create_job(
            "thread",
            "mutator",
            "printf changed > src/lib.rs",
            "runtime",
            "/lease",
        )
        .await
        .unwrap();
    state
        .finish_job("mutator", "runtime", "completed", "exit code: 0")
        .await
        .unwrap();
    assert!(epoch(&state).await > before);
    deliver_jobs(&state).await;
    state
        .set_goal_status("thread", GoalStatus::Active, None)
        .await
        .unwrap();
    request(&state, &goal, &old).await;
    assert_eq!(
        verify(&state, "thread")
            .await
            .unwrap()
            .unwrap()
            .verification,
        "gaps"
    );
}
