//! Goals: rounds, the review of evidence, cancelling and drafts.

use super::*;

#[tokio::test]
async fn a_goal_goes_on_round_after_round_until_they_run_out() {
    let state = thread_state("goal_rounds").await;
    let config = Arc::new(temp_live_config_with(
        "goal_config",
        "limits:\n  max_goal_rounds: 2\n",
    ));
    let draft = state.set_goal("thread", "build it").await.unwrap();
    let draft = state
        .propose_goal(
            "thread",
            &draft.revision,
            vec![crate::state::GoalCriterion {
                id: "result".into(),
                description: "review result".into(),
                check: crate::state::GoalCheck::UserConfirmation,
            }],
        )
        .await
        .unwrap();
    let approve = format!("/goal approve {}", draft.revision);
    let answer = vec![Step::Text("working"), Step::Done];
    // One more for the title the thread gets after its first turn.
    let provider = provider(vec![answer.clone(); 4]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send(approve).await.unwrap();
        loop {
            if let Some(ThreadEvent::Goal {
                goal: Some(goal), ..
            }) = output.next_event().await
                && goal.status == GoalStatus::Paused
                && goal.approved
            {
                return goal;
            }
        }
    };

    let (_, goal) = tokio::join!(runner.run(), client);
    assert_eq!(goal.rounds, 2);
    assert_eq!(goal.note.as_deref(), Some("all 2 rounds used"));
    let users: Vec<_> = transcript(&state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::User)
        .map(|(_, text)| text)
        .collect();
    assert_eq!(
        users,
        [
            "Goal: build it",
            "Continue toward the goal (round 1 of 2): build it",
            "Continue toward the goal (round 2 of 2): build it",
        ]
    );
    let requests = provider.requests.lock().unwrap();
    assert!(matches!(
        requests[0].last(),
        Some(ChatMessage::System(text)) if text.starts_with("Standing goal")
    ));
}

async fn verified_fixture(state: &State) -> (Goal, String) {
    let goal = state.set_goal("thread", "build it").await.unwrap();
    let goal = state
        .propose_goal(
            "thread",
            &goal.revision,
            vec![crate::state::GoalCriterion {
                id: "exit".into(),
                description: "approved check".into(),
                check: crate::state::GoalCheck::Command {
                    command: "exit 0".into(),
                },
            }],
        )
        .await
        .unwrap();
    let goal = state.approve_goal("thread", &goal.revision).await.unwrap();
    state
        .add_user_message("thread", "proof", "run the approved check")
        .await
        .unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    state
        .record_tool_calls(
            &turn.turn_id,
            crate::state::TurnAnswer {
                content: String::new(),
                reasoning: None,
            },
            vec![ToolCall {
                id: "check".into(),
                name: "run_command".into(),
                arguments: json!({"command":"exit 0"}),
            }],
        )
        .await
        .unwrap();
    let root = temp_dir("goal-real-proof");
    std::fs::create_dir_all(&root).unwrap();
    let scope = ToolScope::new(&root, &root).unwrap();
    let tool = crate::tools::RunCommand::default();
    let result = tool
        .call_result(
            json!({"command":"exit 0"}),
            crate::testing::tool_context(&scope),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    let result = state
        .record_tool_result_with_outcome(
            &turn.turn_id,
            "check",
            result.content,
            result.artifacts,
            "succeeded",
            result.exit_code,
        )
        .await
        .unwrap();
    state
        .finish_turn(
            &turn.turn_id,
            crate::state::TurnOutcome::Completed(crate::state::TurnAnswer {
                content: "check executed".into(),
                reasoning: None,
            }),
        )
        .await
        .unwrap();
    (goal, format!("tool:{}", result.message_id))
}

#[tokio::test]
async fn runner_routes_a_completion_request_through_saved_evidence_review() {
    let state = thread_state("goal-runner-review").await;
    let (goal, result_id) = verified_fixture(&state).await;
    let config = live_config("goal-runner-review-config");
    let provider = provider(vec![
        vec![
            Step::Tool(
                "goal".into(),
                json!({"action":"request_verification","revision":goal.revision,
            "evidence":[{"criterion_id":"exit","result_id":result_id}]}),
            ),
            Step::Done,
        ],
        vec![Step::Text("I think it is done"), Step::Done],
        vec![Step::Text("thread name"), Step::Done],
    ]);
    let (runner, input, mut output) = runner_with(
        &state,
        &provider,
        &config,
        model(None),
        Box::new(crate::tools::GoalTool::new(
            Arc::clone(&state),
            "thread".into(),
        )),
    );
    let client = async move {
        input.send("finish the goal").await.unwrap();
        while let Some(event) = output.next_event().await {
            if let ThreadEvent::Goal {
                goal: Some(goal), ..
            } = event
                && goal.status == GoalStatus::Done
            {
                assert_eq!(goal.verification, "passed");
                return;
            }
        }
        panic!("runtime did not publish verified completion");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(), client);
    })
    .await
    .unwrap();
    assert_eq!(
        state.goal("thread").await.unwrap().unwrap().status,
        GoalStatus::Done
    );
}

#[tokio::test]
async fn cancelling_after_the_model_requests_verification_prevents_completion() {
    let state = thread_state("goal-runner-cancel").await;
    let (goal, result_id) = verified_fixture(&state).await;
    let config = live_config("goal-runner-cancel-config");
    let provider = provider(vec![
        vec![
            Step::Tool(
                "goal".into(),
                json!({"action":"request_verification","revision":goal.revision,
            "evidence":[{"criterion_id":"exit","result_id":result_id}]}),
            ),
            Step::Done,
        ],
        vec![Step::Hang],
    ]);
    let (runner, input, mut output) = runner_with(
        &state,
        &provider,
        &config,
        model(None),
        Box::new(crate::tools::GoalTool::new(
            Arc::clone(&state),
            "thread".into(),
        )),
    );
    let client = async move {
        input.send("finish the goal").await.unwrap();
        while let Some(event) = output.next_event().await {
            if matches!(event, ThreadEvent::ToolFinished { .. }) {
                input.cancel().await.unwrap();
            }
            if let ThreadEvent::TurnFinished(turn) = event {
                assert_eq!(turn.status, TurnStatus::Interrupted);
                return;
            }
        }
        panic!("cancel did not finish the turn");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(), client);
    })
    .await
    .unwrap();
    let goal = state.goal("thread").await.unwrap().unwrap();
    assert_eq!(goal.status, GoalStatus::Paused);
    assert_ne!(goal.verification, "passed");
}

#[tokio::test]
async fn a_draft_blocks_non_goal_tools_even_when_the_model_calls_them() {
    let state = thread_state("goal-draft-block").await;
    state.set_goal("thread", "do work").await.unwrap();
    let config = live_config("goal-draft-block-config");
    let provider = provider(vec![
        vec![Step::Call("do work"), Step::Done],
        vec![Step::Text("criteria needed"), Step::Done],
        vec![Step::Text("title"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("propose criteria").await.unwrap();
        next_turn_finished(&mut output).await;
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(), client);
    })
    .await
    .unwrap();
    let outcome: String =
        sqlx::query_scalar("SELECT outcome FROM tool_executions WHERE call_id = 'call_1'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(outcome, "failed");
    assert!(
        state
            .list_messages("thread")
            .await
            .unwrap()
            .iter()
            .any(|m| m.role == MessageRole::Tool && m.content.contains("execution is blocked"))
    );
}
