//! `goal`: the model's side of the standing goal, through the same `apply` as `/goal`.
//! It may set one for longer work, pause it when it needs the user, and must mark it
//! done when it is.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext, builtin::parse_arguments};
use crate::{
    BoxFuture,
    chat::goal::{self, GoalAction},
    runtime::ThreadEvent,
    state::State,
};

pub struct GoalTool {
    state: Arc<State>,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    text: Option<String>,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Show,
    Set,
    Pause,
    Resume,
    Clear,
    Done,
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

    /// The reminder of an active goal names this tool, so every mode has it.
    fn switchable(&self) -> bool {
        false
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let rounds = context.config.limits().max_goal_rounds;
        format!(
            "The standing goal of this chat. While a goal is active, each turn that ends \
             without it done starts another, up to {rounds} rounds. Set one (text) for work \
             that takes many turns; mark it done (note: a short result) as soon as it is \
             fully done; pause it (note: why) when you need the user; resume, clear or show \
             it. The user can do the same with /goal."
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["show", "set", "pause", "resume", "clear", "done"]
                },
                "text": { "type": "string", "description": "For set: the goal." },
                "note": {
                    "type": "string",
                    "description": "For done: the result. For pause: the reason."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let action = match arguments.action {
                Action::Show => GoalAction::Show,
                Action::Set => GoalAction::Set(arguments.text.unwrap_or_default()),
                Action::Pause => GoalAction::Pause(arguments.note),
                Action::Resume => GoalAction::Resume,
                Action::Clear => GoalAction::Clear,
                Action::Done => GoalAction::Done(arguments.note),
            };
            let (goal, note) = goal::apply(&self.state, &self.thread_id, action).await?;
            let answer = match &goal {
                Some(goal) => goal::describe(goal),
                None => note.clone(),
            };
            if let Some(progress) = context.progress {
                progress.emit(ThreadEvent::Goal {
                    goal,
                    note,
                    started: None,
                });
            }
            Ok(answer)
        })
    }
}
