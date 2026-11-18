//! The smaller sections of the config: approvals, memory, limits, logging, appearance
//! and what threads record.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use dusk_protocol::config::yes;
pub use dusk_protocol::config::{ApprovalPolicy, Memory, WorkHistory};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadConfig {
    #[serde(default)]
    pub record: RecordMode,
}

/// Token limits for every model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Older configs set these; the model's window and output cap replace them.
    #[serde(default, skip_serializing)]
    max_input_tokens: Option<u64>,
    #[serde(default, skip_serializing)]
    max_output_tokens: Option<u64>,
    /// Maximum number of subagents running across all threads.
    #[serde(default = "default_max_agents_at_work")]
    pub max_agents_at_work: usize,
    /// Turns a goal may start by itself before it is paused for the user.
    #[serde(default = "default_max_goal_rounds")]
    pub max_goal_rounds: u32,
    /// Seconds a model call may stay silent before it is given up on. A reasoning model
    /// can think for a while, so the quiet counts, not the whole call.
    #[serde(default = "default_model_timeout_seconds")]
    pub model_timeout_seconds: u32,
    /// Tries of one model call that wrote nothing to the thread, the first included.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Seconds a model call that cannot reach its provider, or gets no answer in time, is
    /// tried again for, from its first attempt. A short network drop costs no turn; other
    /// failures keep to `max_attempts`.
    #[serde(default = "default_retry_unreachable_seconds")]
    pub retry_unreachable_seconds: u32,
    /// Seconds after its last activity within which a turn a restart cut off goes on by
    /// itself. An older one asks the user first, so opening an old chat spends no tokens
    /// unasked.
    #[serde(default = "default_resume_within_seconds")]
    pub resume_within_seconds: u32,
    /// Whether a turn whose provider cannot serve it now (rate limited, out of quota, out
    /// of reach, overloaded) waits and goes on by itself, for up to a day, instead of
    /// failing once the call's attempts are spent. The wait outlasts a restart.
    #[serde(default = "yes")]
    pub wait_for_provider: bool,
    /// Whether a compaction whose own request is too large for the model summarizes the
    /// older half of the context instead. Off until the evals show it helps.
    #[serde(default)]
    pub shrink_context: bool,
    /// The ladder for the same error coming back within a turn through calls that
    /// differ: a note, then a re-plan, then a stop. Off until the evals show it does not
    /// lower the pass rate.
    #[serde(default)]
    pub failure_ladder: bool,
    /// Whether `read_file` also takes `paths`, to read several files in one call. It
    /// changes a tool the model reads, so it is off until the evals show it saves steps.
    #[serde(default)]
    pub read_many_files: bool,
    /// Whether the read-only calls of one step run together instead of one by one. The
    /// model sees the same results in the same order; off is for comparing runs.
    #[serde(default = "yes")]
    pub parallel_reads: bool,
    /// Whether each tool's description stays as a thread first showed it until the
    /// thread's next compaction, so a memory rewritten, or a skill or an agent added,
    /// mid-thread does not change the start of its requests and the provider's cache of
    /// it holds. Off until the evals show it does not lower the pass rate.
    #[serde(default)]
    pub stable_prefix: bool,
    /// Times the same error comes back before the model is told to stop varying the call.
    #[serde(default = "default_failure_note_at")]
    pub failure_note_at: u32,
    /// Times before the model is asked for a different plan.
    #[serde(default = "default_failure_replan_at")]
    pub failure_replan_at: u32,
    /// Times before the turn ends as failed.
    #[serde(default = "default_failure_stop_at")]
    pub failure_stop_at: u32,
    /// Whether the files a command changes in the project are recorded, with a diff when
    /// the text before is known, so the agent and clients can ask what changed. Off, no
    /// command is watched and the chat's list of changed files stays empty too.
    #[serde(default = "yes")]
    pub record_changes: bool,
    /// Whether old tool results are sent as short stubs that say how to get them back,
    /// instead of word for word on every request. The full text stays stored. Off until
    /// the evals show the model does not run again what it no longer sees.
    #[serde(default)]
    pub stub_results: bool,
    /// Steps, newest first, whose tool results are always sent whole.
    #[serde(default = "default_stub_keep_steps")]
    pub stub_keep_steps: u32,
    /// Tool results smaller than this, in estimated tokens, are always sent whole: their
    /// stub would save little.
    #[serde(default = "default_stub_min_tokens")]
    pub stub_min_tokens: u64,
    /// Whether a compaction summarizes only the older part of the context and keeps the
    /// newest messages word for word, so the step in hand is not retold from a summary.
    /// Off until the evals show it does not lower the pass rate.
    #[serde(default)]
    pub compact_tail: bool,
    /// The most a kept tail holds, in estimated tokens; never more than a fifth of the
    /// model's input limit, so a compaction always frees most of it.
    #[serde(default = "default_compact_tail_tokens")]
    pub compact_tail_tokens: u64,
    /// Whether the context is also compacted between turns once it passes 60% of the input
    /// limit, so the summary is written in the pause and not in the middle of a turn. Off
    /// until the evals show the cached share and the wall time do not get worse.
    #[serde(default)]
    pub compact_early: bool,
    /// Whether the output of a command whose program is known (cargo, pytest, npm, git
    /// transfers) is sent with passing tests, progress and repeats left out, and a note
    /// on how to read it whole. The full output stays stored. Off until the evals show
    /// the model does not miss what it no longer sees.
    #[serde(default)]
    pub shape_output: bool,
    /// Whether the core keeps an index of a saved project's source (files, symbols, the
    /// names each file uses) for the `project_graph` tool. The model sees nothing of it.
    /// Off until the indexer's benchmark meets its budget.
    #[serde(default)]
    pub project_graph: bool,
    /// Whether the core writes down what ended a problem in a saved project (a repeated
    /// error that then passed, a verification that passed) for the `recall` tool. Off
    /// until the evals show recalled notes help.
    #[serde(default)]
    pub episodes: bool,
    /// Whether a call that ends a repeated failure asks the model for a one-line lesson
    /// at the end of its answer, stored with the episode. Off until the evals show the
    /// lesson adds to the facts.
    #[serde(default)]
    pub episode_lesson: bool,
    /// Whether the chat in ORCHESTRATOR mode gets the plan form of `tasks` (tasks that
    /// depend on each other, and `run`, which has the core start the team). Off until the
    /// evals show the lead needs fewer steps (`docs/plan-and-teams.md`).
    #[serde(default)]
    pub plan_graph: bool,
    /// Whether a team member gets its own prompt, tool set, task card and a one-line
    /// hand-in, instead of today's subagent prompt. Off until the evals show it helps.
    #[serde(default)]
    pub task_packets: bool,
    /// Whether each writing member works in its own git worktree and branch. Off until
    /// the evals show the pass rate does not fall.
    #[serde(default)]
    pub member_worktrees: bool,
    /// Whether members of a team may message each other (`message`, and `task` for
    /// them). Off until the evals show it helps.
    #[serde(default)]
    pub peer_messages: bool,
    /// Whether a task may name a procedure that hands its steps out one at a time. Off
    /// until the evals show it helps.
    #[serde(default)]
    pub procedures: bool,
    /// Members of one team that work at once; never above `max_agents_at_work`.
    #[serde(default = "default_team_at_once")]
    pub team_at_once: usize,
    /// The most model steps a team may spend before it ends.
    #[serde(default = "default_team_max_steps")]
    pub team_max_steps: u32,
    /// The most tokens a team may spend, prompt and completion together.
    #[serde(default = "default_team_max_tokens")]
    pub team_max_tokens: u64,
    /// The most minutes a team may run.
    #[serde(default = "default_team_max_minutes")]
    pub team_max_minutes: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: None,
            max_output_tokens: None,
            max_agents_at_work: default_max_agents_at_work(),
            max_goal_rounds: default_max_goal_rounds(),
            model_timeout_seconds: default_model_timeout_seconds(),
            max_attempts: default_max_attempts(),
            retry_unreachable_seconds: default_retry_unreachable_seconds(),
            resume_within_seconds: default_resume_within_seconds(),
            wait_for_provider: true,
            shrink_context: false,
            failure_ladder: false,
            read_many_files: false,
            parallel_reads: true,
            stable_prefix: false,
            failure_note_at: default_failure_note_at(),
            failure_replan_at: default_failure_replan_at(),
            failure_stop_at: default_failure_stop_at(),
            record_changes: true,
            stub_results: false,
            stub_keep_steps: default_stub_keep_steps(),
            stub_min_tokens: default_stub_min_tokens(),
            compact_tail: false,
            compact_tail_tokens: default_compact_tail_tokens(),
            compact_early: false,
            shape_output: false,
            project_graph: false,
            episodes: false,
            episode_lesson: false,
            plan_graph: false,
            task_packets: false,
            member_worktrees: false,
            peer_messages: false,
            procedures: false,
            team_at_once: default_team_at_once(),
            team_max_steps: default_team_max_steps(),
            team_max_tokens: default_team_max_tokens(),
            team_max_minutes: default_team_max_minutes(),
        }
    }
}

impl Limits {
    /// Members that may work at once: `team_at_once`, cut to `max_agents_at_work`. Cut, not
    /// refused, so a config that allows two agents still loads with the default of three.
    pub fn team_members_at_once(&self) -> usize {
        self.team_at_once.min(self.max_agents_at_work)
    }
}

fn default_max_agents_at_work() -> usize {
    8
}

fn default_max_goal_rounds() -> u32 {
    10
}

/// Long enough for a reasoning model's pauses, short enough not to hang a turn.
pub const DEFAULT_MODEL_TIMEOUT_SECONDS: u32 = 120;
/// The floor and the ceiling a model call's quiet can be set to.
pub const MIN_MODEL_TIMEOUT_SECONDS: u32 = 5;
pub const MAX_MODEL_TIMEOUT_SECONDS: u32 = 3600;
/// One try is a valid choice: the user asked for no retries.
pub const MIN_ATTEMPTS: u32 = 1;
pub const MAX_ATTEMPTS: u32 = 20;
/// Long enough for a router or Wi-Fi to come back; one hour at most, so a turn ends.
pub const DEFAULT_RETRY_UNREACHABLE_SECONDS: u32 = 300;
pub const MIN_RETRY_UNREACHABLE_SECONDS: u32 = 10;
pub const MAX_RETRY_UNREACHABLE_SECONDS: u32 = 3600;

fn default_model_timeout_seconds() -> u32 {
    DEFAULT_MODEL_TIMEOUT_SECONDS
}

fn default_max_attempts() -> u32 {
    6
}

fn default_retry_unreachable_seconds() -> u32 {
    DEFAULT_RETRY_UNREACHABLE_SECONDS
}

/// An hour: a restart in the middle of work resumes it, a chat left for the day asks.
pub const DEFAULT_RESUME_WITHIN_SECONDS: u32 = 3600;
/// A minute at least, so a quick restart never asks; a week at most.
pub const MIN_RESUME_WITHIN_SECONDS: u32 = 60;
pub const MAX_RESUME_WITHIN_SECONDS: u32 = 7 * 24 * 3600;

fn default_resume_within_seconds() -> u32 {
    DEFAULT_RESUME_WITHIN_SECONDS
}

/// Two of the same error is normal; the ladder starts later, and stops before a turn
/// burns itself out.
pub const MIN_FAILURE_RUNG: u32 = 2;
pub const MAX_FAILURE_RUNG: u32 = 50;

fn default_failure_note_at() -> u32 {
    3
}

fn default_failure_replan_at() -> u32 {
    5
}

fn default_failure_stop_at() -> u32 {
    8
}

/// Two steps at least, so the result the model is reading is never put away under it.
pub const MIN_STUB_KEEP_STEPS: u32 = 2;
pub const MAX_STUB_KEEP_STEPS: u32 = 50;
/// Below 50 tokens a stub is about as long as the result.
pub const MIN_STUB_MIN_TOKENS: u64 = 50;
pub const MAX_STUB_MIN_TOKENS: u64 = 4000;

fn default_stub_keep_steps() -> u32 {
    8
}

fn default_stub_min_tokens() -> u64 {
    200
}

/// Below 2,000 tokens a tail holds about one step; above 64,000 it is most of a window.
pub const MIN_COMPACT_TAIL_TOKENS: u64 = 2_000;
pub const MAX_COMPACT_TAIL_TOKENS: u64 = 64_000;

fn default_compact_tail_tokens() -> u64 {
    16_000
}

/// A team's ceilings. The floors keep a plan able to finish one task; the tops keep a
/// mistyped number from being an open-ended bill.
pub const MIN_TEAM_AT_ONCE: usize = 1;
pub const MAX_TEAM_AT_ONCE: usize = 8;
pub const MIN_TEAM_MAX_STEPS: u32 = 10;
pub const MAX_TEAM_MAX_STEPS: u32 = 5_000;
pub const MIN_TEAM_MAX_TOKENS: u64 = 100_000;
pub const MAX_TEAM_MAX_TOKENS: u64 = 100_000_000;
pub const MIN_TEAM_MAX_MINUTES: u32 = 5;
pub const MAX_TEAM_MAX_MINUTES: u32 = 1_440;

fn default_team_at_once() -> usize {
    3
}

fn default_team_max_steps() -> u32 {
    400
}

fn default_team_max_tokens() -> u64 {
    4_000_000
}

fn default_team_max_minutes() -> u32 {
    120
}

/// A role name is an agent's folder name.
const MAX_ROLE_NAME: usize = 64;

/// Which model a team's members answer on, as `provider/model`. The user's alone: a role
/// from a pack must not choose what the user pays for. A member's model is `models[role]`,
/// else `model`, else the chat's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, String>,
}

impl Team {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(super) fn validate(&self) -> anyhow::Result<()> {
        for model in self.model.iter().chain(self.models.values()) {
            let named = model.split_once('/').is_some_and(|(provider, model)| {
                !provider.trim().is_empty() && !model.trim().is_empty()
            });
            anyhow::ensure!(
                named && !model.chars().any(char::is_control),
                "team models must be written as provider/model"
            );
        }
        for role in self.models.keys() {
            anyhow::ensure!(
                !role.is_empty()
                    && role.len() <= MAX_ROLE_NAME
                    && role
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "team.models keys must be role names of up to {MAX_ROLE_NAME} of a-z, 0-9, - and _"
            );
        }
        Ok(())
    }
}

/// What the core records about its work. Off, nothing is written or sent at all.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Sends spans of turns, model calls and tool calls over OTLP/HTTP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traces: Option<Traces>,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            enabled: true,
            traces: None,
        }
    }
}

impl Logging {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// An OpenTelemetry collector. `headers` may hold an API key, so they are never logged.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traces {
    /// The collector's OTLP/HTTP address, such as `http://127.0.0.1:4318`; spans go to
    /// `/v1/traces` under it.
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// How the client looks and behaves on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// Minutes with no input and no work before the idle screen plays; 0 never.
    #[serde(default = "default_idle_screen_minutes")]
    pub idle_screen_minutes: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            idle_screen_minutes: default_idle_screen_minutes(),
        }
    }
}

impl Appearance {
    /// A day is past any idle a user would wait for.
    pub const MAX_IDLE_SCREEN_MINUTES: u32 = 24 * 60;

    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_idle_screen_minutes() -> u32 {
    5
}

/// What a thread stores from the model output. User messages are always stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecordMode {
    /// Everything the model produces: reasoning, tool calls and the answer.
    #[default]
    Full,
    /// Only the answer.
    Answer,
}

impl RecordMode {
    pub fn records_reasoning(self) -> bool {
        match self {
            Self::Full => true,
            Self::Answer => false,
        }
    }
}

pub fn is_true(value: &bool) -> bool {
    *value
}

pub fn is_zero(value: &u64) -> bool {
    *value == 0
}

pub fn is_false(value: &bool) -> bool {
    !value
}
