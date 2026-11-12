//! The agent: runs a turn over a thread step by step. Each step streams the model's
//! response; when it calls tools, they run and the next step sends their results back,
//! until the model answers without calling a tool.

mod calls;
mod pictures;
mod report;
mod step;
#[cfg(test)]
mod tests;
mod turn;

use super::Stop;
use crate::plugins::Hooks;
use crate::runtime::{ThreadEvent, model_choice::ModelChoice};
use crate::state::artifacts::ArtifactStore;
use crate::{
    config::live::LiveConfig,
    config::{Config, Effort},
    providers::{ChatMessage, ModelOutput, ToolSpec},
    state::{State, TurnAnswer, TurnOutcome},
    tools::{ToolRegistry, ToolScope},
};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::Duration};
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

/// A model call that fails before writing anything is tried this many times in all.
const MAX_ATTEMPTS: u32 = 6;

/// Doubles before each further attempt, up to `MAX_RETRY_DELAY`.
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);

const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

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
    /// Where the turn is: planning, carrying out, or working out a surprise.
    pub pace: Mutex<Pace>,
    /// Calls repeated with the same result, to tell a stuck model.
    pub repeats: Mutex<crate::runtime::agent::repeats::Repeats>,
    /// The question on the user's screen. Its answer joins the running turn instead of
    /// waiting for the next one.
    pub asking: Mutex<Option<String>>,
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
}

/// How one step ended.
enum StepEnd {
    Answered(TurnOutcome),
    CalledTools,
    Stopped(Stop, TurnOutcome),
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

/// Exponential backoff with up to a quarter of jitter, so agents that failed together do
/// not all come back at the same moment.
fn retry_delay(attempt: u32) -> Duration {
    let base = FIRST_RETRY_DELAY
        .saturating_mul(2u32.saturating_pow(attempt - 1))
        .min(MAX_RETRY_DELAY);
    let noise = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.subsec_nanos());
    base + base.mul_f64(f64::from(noise % 1000) / 4000.0)
}
