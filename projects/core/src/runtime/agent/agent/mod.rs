//! The agent: runs a turn over a thread step by step. Each step streams the model's
//! response; when it calls tools, they run and the next step sends their results back,
//! until the model answers without calling a tool.

mod calls;
mod episodes;
mod judge;
mod mode;
mod pictures;
mod report;
mod resume;
mod retry;
mod shaped;
mod step;
mod stubs;
#[cfg(test)]
mod tests;
mod turn;
mod wait;

pub use resume::{Decision, Recovered};
pub(crate) use turn::Admit;

use super::Stop;
use crate::plugins::Hooks;
use crate::runtime::{ThreadEvent, model_choice::ModelChoice};
use crate::state::artifacts::ArtifactStore;
use crate::{
    config::live::LiveConfig,
    config::{Config, Effort},
    providers::{ChatMessage, ModelOutput, ProviderError, ToolSpec},
    state::{State, TurnAnswer, TurnFailure, TurnOutcome},
    tools::{ToolRegistry, ToolScope},
};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc;

/// The rhythm of a turn: think once, then act. The first step plans and a step after
/// a failed tool works out why, both at the effort set; a step after tools that went
/// as expected only carries out the plan, at a light effort, so the model does not
/// reason at length before each obvious next call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pace {
    #[default]
    Plan,
    Follow,
    Rethink,
}

impl Pace {
    pub fn effort(self, set: Option<Effort>) -> Option<Effort> {
        match self {
            Self::Follow => set.map(Effort::light),
            Self::Plan | Self::Rethink => set,
        }
    }
}

pub struct Agent {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub tools: Arc<ToolRegistry>,
    pub scope: Arc<ToolScope>,
    /// Comes right after the prompt in every request.
    pub project: String,
    /// The model that refused pictures; the others still get them.
    pub pictures_refused: Mutex<Option<String>>,
    /// The model that refused a subagent's reasoning effort; it is asked without one.
    pub effort_refused: Mutex<Option<String>>,
    /// The model that refused the loop judge's effort; the judge asks it without one.
    pub judge_effort_refused: Mutex<Option<String>>,
    /// Where the turn is: planning, carrying out, or working out a surprise.
    pub pace: Mutex<Pace>,
    /// Calls repeated with the same result, to tell a stuck model.
    pub repeats: Mutex<crate::runtime::agent::repeats::Repeats>,
    /// The questions on the user's screen. Their answer joins the running turn instead
    /// of waiting for the next one.
    pub asking: Mutex<Vec<String>>,
    pub thread_id: String,
    /// COMPACT.md: how to summarize the thread.
    pub compact_prompt: PathBuf,
    /// FORK.md: how to describe the thread for a new one to go on from.
    pub fork_prompt: PathBuf,
    /// The prompts of the chat's modes.
    pub modes_dir: PathBuf,
    pub artifacts: ArtifactStore,
    /// How many agents are above this thread; 0 for a chat.
    pub depth: usize,
    pub events: mpsc::UnboundedSender<ThreadEvent>,
    /// What plugins run when something happens here.
    pub hooks: Arc<Hooks>,
    /// How each provider's calls go, shared by every thread.
    pub health: Arc<crate::providers::Health>,
    /// Notices the skills a writing call changed.
    pub skill_watch: Arc<crate::catalog::history::Watch>,
    /// Where those changes are kept: the main state, also for a temporary chat, because
    /// a skill's history belongs to the skill and not to the thread.
    pub skill_state: Arc<crate::state::State>,
}

/// How one step ended.
enum StepEnd {
    Answered(TurnOutcome),
    CalledTools,
    Stopped(Stop, TurnOutcome),
    /// The provider cannot serve the step now and nothing of it was written: the turn
    /// waits and takes the step again, or fails as the failure says.
    Wait(ProviderError, TurnFailure),
    /// The context does not fit the model: the turn compacts it and takes the step
    /// again, or fails as the failure says.
    TooLarge(TurnFailure),
    /// The model wrote a tool call as text that could not be run: the turn takes the
    /// step again with this note, or ends with the answer as it is.
    Stranded(String, TurnAnswer),
    /// The model's text went round and was cut: the turn takes the step again with this
    /// note.
    Chanted(String),
}

/// What a stopped step keeps: the text written so far, if any.
fn interrupted(answer: TurnAnswer) -> TurnOutcome {
    let written = !answer.content.trim().is_empty();
    TurnOutcome::Interrupted(written.then_some(answer))
}

fn recorded_reasoning(config: &Config, output: &ModelOutput) -> Option<String> {
    let keep = config.thread().record.records_reasoning() && !output.reasoning.trim().is_empty();
    keep.then(|| output.reasoning.clone())
}

/// A model copies the calls it sees in the thread. After a switch to a mode with fewer
/// tools, it is told which of those it no longer has, so it does not reach for them.
/// The project's text goes right after the leading system messages. Compaction places it
/// the same way, so its request starts with the prefix the steps already cached.
pub(super) fn insert_project(messages: &mut Vec<ChatMessage>, project: &str) {
    if project.is_empty() {
        return;
    }
    let after_prompt = messages
        .iter()
        .position(|message| !matches!(message, ChatMessage::System(_)))
        .unwrap_or(messages.len());
    messages.insert(after_prompt, ChatMessage::System(project.to_owned()));
}

fn missing_tools(messages: &[ChatMessage], tools: &[ToolSpec]) -> Option<String> {
    let mut missing: Vec<&str> = messages
        .iter()
        .filter_map(|message| match message {
            ChatMessage::Assistant { tool_calls, .. } => Some(tool_calls),
            _ => None,
        })
        .flatten()
        .map(|call| call.name.as_str())
        .filter(|name| !tools.iter().any(|tool| tool.name == *name))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "Earlier in this thread there are calls to tools you do not have now, since the \
         mode or the tool switches changed: {}. Do not call them; work with the tools you \
         have, or ask an agent that has them.",
        missing.join(", ")
    ))
}
