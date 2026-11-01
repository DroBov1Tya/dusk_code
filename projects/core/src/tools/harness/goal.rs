//! Executor access to a goal: propose a draft or submit saved evidence. User and
//! runtime authority are deliberately absent from the schema and handler.

use std::sync::Arc;

use anyhow::{Context, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    chat::goal,
    runtime::ThreadEvent,
    state::{GoalCriterion, GoalEvidence, State},
    tools::{Tool, ToolContext, builtin::parse_arguments},
};

pub struct GoalTool {
    state: Arc<State>,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    revision: Option<String>,
    criteria: Option<Vec<GoalCriterion>>,
    evidence: Option<Vec<GoalEvidence>>,
    result_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Show,
    Propose,
    RequestVerification,
}

impl GoalTool {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}
impl Tool for GoalTool {
    fn name(&self) -> &'static str {
        "goal"
    }
    fn root_only(&self) -> bool {
        true
    }
    fn switchable(&self) -> bool {
        false
    }
    fn description(&self, _: &ToolContext<'_>) -> String {
        "Inspect the standing goal, its immutable user-approved acceptance contract, saved result IDs and verification gaps. show with result_id reads the saved output. Propose criteria only for an existing unapproved draft, using its revision. Semantic/subjective claims must use user_confirmation; command checks must name the exact command the user approves. request_verification submits the current revision plus one real saved result_id per criterion (tool:<result message ID> or job:<job ID>). It only requests runtime review after the turn and dependencies finish. You cannot approve/change an approved contract, pause/resume/clear a goal, confirm for the user or set done. Failed, cancelled, stale, foreign or untyped results cannot pass.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type":"object", "properties": {
            "action":{"type":"string","enum":["show","propose","request_verification"]},
            "revision":{"type":"string"},
            "result_id":{"type":"string","description":"For show: inspect saved output."},
            "criteria":{"type":"array","minItems":1,"maxItems":20,"items":{
                "type":"object","properties":{
                    "id":{"type":"string"},"description":{"type":"string"},
                    "check":{"oneOf":[
                        {"type":"object","properties":{"kind":{"const":"command"},"command":{"type":"string"}},"required":["kind","command"],"additionalProperties":false},
                        {"type":"object","properties":{"kind":{"const":"user_confirmation"}},"required":["kind"],"additionalProperties":false}
                    ]}
                },"required":["id","description","check"],"additionalProperties":false}},
            "evidence":{"type":"array","maxItems":20,"items":{"type":"object","properties":{
                "criterion_id":{"type":"string"},"result_id":{"type":"string"}
            },"required":["criterion_id","result_id"],"additionalProperties":false}}
        },"required":["action"],"additionalProperties":false})
    }
    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let args: Arguments = parse_arguments(arguments)?;
            if matches!(args.action, Action::Show) {
                if let Some(id) = args.result_id {
                    return self.state.inspect_goal_result(&self.thread_id, &id).await;
                }
                let goal = self
                    .state
                    .goal(&self.thread_id)
                    .await?
                    .context("no goal; only the user creates one with /goal <text>")?;
                return Ok(format!(
                    "{}\nSaved results:\n{}",
                    goal::describe(&goal),
                    self.state.goal_result_index(&self.thread_id).await?
                ));
            }
            ensure!(args.result_id.is_none(), "result_id is only for show");
            let revision = args.revision.context("revision is required")?;
            let goal = match args.action {
                Action::Propose => {
                    ensure!(args.evidence.is_none(), "propose does not accept evidence");
                    self.state
                        .propose_goal(
                            &self.thread_id,
                            &revision,
                            args.criteria.context("criteria are required")?,
                        )
                        .await?
                }
                Action::RequestVerification => {
                    ensure!(
                        args.criteria.is_none(),
                        "approved criteria cannot be changed"
                    );
                    let call = context.progress.context("no recorded call")?;
                    self.state
                        .request_goal_verification(
                            &self.thread_id,
                            &revision,
                            call.call_id,
                            args.evidence.context("evidence is required")?,
                        )
                        .await?
                }
                Action::Show => unreachable!(),
            };
            let note = goal::describe(&goal);
            if let Some(progress) = context.progress {
                progress.emit(ThreadEvent::Goal {
                    goal: Some(goal),
                    note: note.clone(),
                    started: None,
                });
            }
            Ok(note)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        testing::{temp_dir, temp_state, tool_context},
        tools::ToolScope,
    };

    #[tokio::test]
    async fn model_has_no_approval_edit_confirmation_or_completion_authority() {
        let state = Arc::new(temp_state("goal-model-authority").await);
        state.create_thread("thread", "test").await.unwrap();
        let goal = state.set_goal("thread", "do work").await.unwrap();
        let tool = GoalTool::new(Arc::clone(&state), "thread".into());
        let root = temp_dir("goal-model-scope");
        std::fs::create_dir_all(&root).unwrap();
        let scope = ToolScope::new(&root, &root).unwrap();
        let actions = tool.parameters()["properties"]["action"]["enum"].clone();
        for action in [
            "done", "clear", "set", "approve", "confirm", "pause", "resume",
        ] {
            assert!(!actions.as_array().unwrap().iter().any(|v| v == action));
            assert!(
                tool.call(json!({"action":action}), tool_context(&scope))
                    .await
                    .is_err()
            );
        }
        assert_eq!(state.goal("thread").await.unwrap().unwrap(), goal);
        assert!(tool.root_only());
    }
}
