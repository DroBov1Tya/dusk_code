use super::evidence::read_evidence;
use super::*;
use crate::state::{Goal, GoalCheck, GoalCriterion, GoalEvidence};
use crate::{
    providers::ToolCall,
    state::{GoalStatus, Turn, TurnAnswer, TurnOutcome},
    testing::temp_state,
};
use serde_json::json;
use uuid::Uuid;

fn criterion(check: GoalCheck) -> GoalCriterion {
    GoalCriterion {
        id: "check".into(),
        description: "accepted behavior".into(),
        check,
    }
}
fn answer() -> TurnAnswer {
    TurnAnswer {
        content: "reported complete".into(),
        reasoning: None,
    }
}
async fn approved(name: &str, check: GoalCheck) -> (State, Goal) {
    let state = temp_state(name).await;
    state.create_thread("thread", "test").await.unwrap();
    let goal = state
        .set_goal("thread", "implement behavior")
        .await
        .unwrap();
    let goal = state
        .propose_goal("thread", &goal.revision, vec![criterion(check)])
        .await
        .unwrap();
    let goal = state.approve_goal("thread", &goal.revision).await.unwrap();
    (state, goal)
}
async fn turn(state: &State) -> Turn {
    state
        .add_user_message("thread", &Uuid::now_v7().to_string(), "work")
        .await
        .unwrap();
    state.start_turn("thread").await.unwrap().unwrap()
}
async fn result(
    state: &State,
    name: &str,
    args: serde_json::Value,
    outcome: &str,
    code: Option<i32>,
    completed: bool,
) -> String {
    let turn = turn(state).await;
    state
        .record_tool_calls(
            &turn.turn_id,
            answer(),
            vec![ToolCall {
                id: "result".into(),
                name: name.into(),
                arguments: args,
            }],
        )
        .await
        .unwrap();
    let message = state
        .record_tool_result_with_outcome(
            &turn.turn_id,
            "result",
            "saved real output".into(),
            Vec::new(),
            outcome,
            code,
        )
        .await
        .unwrap();
    let end = if completed {
        TurnOutcome::Completed(answer())
    } else {
        TurnOutcome::Interrupted(None)
    };
    state.finish_turn(&turn.turn_id, end).await.unwrap();
    format!("tool:{}", message.message_id)
}
async fn request(state: &State, goal: &Goal, id: &str) {
    let turn = turn(state).await;
    state
        .record_tool_calls(
            &turn.turn_id,
            answer(),
            vec![ToolCall {
                id: "request".into(),
                name: "goal".into(),
                arguments: json!({"action":"request_verification"}),
            }],
        )
        .await
        .unwrap();
    state
        .request_goal_verification(
            "thread",
            &goal.revision,
            "request",
            vec![GoalEvidence {
                criterion_id: "check".into(),
                result_id: id.into(),
            }],
        )
        .await
        .unwrap();
    state
        .record_tool_result_with_outcome(
            &turn.turn_id,
            "request",
            "review requested".into(),
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
}
fn command() -> GoalCheck {
    GoalCheck::Command {
        command: "cargo test --locked".into(),
    }
}
async fn proof(state: &State) -> String {
    result(
        state,
        "run_command",
        json!({"command":"cargo test --locked"}),
        "succeeded",
        Some(0),
        true,
    )
    .await
}

mod blocking;
mod epochs;
mod proofs;
