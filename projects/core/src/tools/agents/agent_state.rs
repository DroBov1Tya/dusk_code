//! `agent_state`: an agent says where it is in its work. Each task starts in planning;
//! the agent moves on to executing, reviewing and done, or pauses. The asker and the
//! user see the phase, and a crash leaves it on record.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    runtime::ThreadEvent,
    state::{AgentPhase, State},
    tools::{Tool, ToolContext, ToolKind, schema, schema::parse_arguments},
};

pub struct AgentState {
    state: Arc<State>,
    thread_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AgentStateArguments {
    phase: AgentPhase,
    /// What is happening, in a few words, for the asker to see.
    note: Option<String>,
}

impl AgentState {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}

impl Tool for AgentState {
    fn name(&self) -> &'static str {
        "agent_state"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Agents
    }

    fn switchable(&self) -> bool {
        false
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Say where you are in your task: planning (reading and deciding how), executing \
         (doing the work), reviewing (checking what you did against the task), done, or \
         paused (waiting on something). Work goes planning, executing, reviewing, done; \
         from reviewing you may go back to executing to fix what the review found. \
         Optional: the runtime starts the task in planning and marks it done when you \
         answer; call this only to pause or to show progress in a long task."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<AgentStateArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: AgentStateArguments = parse_arguments(arguments)?;
            let note = arguments.note.unwrap_or_default();
            self.state
                .move_agent_phase(&self.thread_id, arguments.phase, &note)
                .await?;
            if let Some(progress) = context.progress {
                progress.emit(ThreadEvent::AgentPhase {
                    phase: arguments.phase,
                    note: note.clone(),
                });
            }
            Ok(format!("phase: {}", arguments.phase.name()))
        })
    }
}
