//! The smaller sections of the config: approvals, memory, limits, logging, appearance
//! and what threads record.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use dusk_protocol::config::yes;
pub use dusk_protocol::config::{ApprovalPolicy, Memory, WorkHistory};
use dusk_protocol::thread::{
    AUTO_COMPACT_PERCENT, MAX_AUTO_COMPACT_PERCENT, MIN_AUTO_COMPACT_PERCENT,
};

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
    /// Older configs capped a goal's rounds; a goal now runs until it is met or the user
    /// bounds it in its own words.
    #[serde(default, skip_serializing)]
    max_goal_rounds: Option<u32>,
    /// Maximum number of subagents running across all threads.
    #[serde(default = "default_max_agents_at_work")]
    pub max_agents_at_work: usize,
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
    /// older half of the context instead.
    #[serde(default = "yes")]
    pub shrink_context: bool,
    /// The ladder for the same error coming back within a turn through calls that
    /// differ: a note, then a re-plan, then a stop.
    #[serde(default = "yes")]
    pub failure_ladder: bool,
    /// Whether `read_file` also takes `paths`, to read several files in one call. It
    /// changes a tool the model reads.
    #[serde(default = "yes")]
    pub read_many_files: bool,
    /// Whether the read-only calls of one step run together instead of one by one. The
    /// model sees the same results in the same order; off is for comparing runs.
    #[serde(default = "yes")]
    pub parallel_reads: bool,
    /// Whether each tool's description stays as a thread first showed it until the
    /// thread's next compaction, so a memory rewritten, or a skill or an agent added,
    /// mid-thread does not change the start of its requests and the provider's cache of
    /// it holds.
    #[serde(default = "yes")]
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
    /// Whether calls going in circles get a note and then a request for a different
    /// plan: a short run of calls coming round with the same results, or a read giving
    /// the same result with nothing changed between. Off, only the same call with the
    /// same result three times in a row gets a note. Notes only: it never ends a turn.
    #[serde(default = "yes")]
    pub loop_cycles: bool,
    /// Rounds of a cycle (or times a read came back the same) before the note.
    #[serde(default = "default_cycle_note_at")]
    pub cycle_note_at: u32,
    /// Rounds before the model is asked for a different plan.
    #[serde(default = "default_cycle_replan_at")]
    pub cycle_replan_at: u32,
    /// Whether a test run repeating one of its runner (the same command, or the same
    /// tests failing) with no project file changed since gets a note with what that run
    /// found, and a run's failing test ids stand for its
    /// error in the failure ladder. The command still runs. Notes only.
    #[serde(default = "yes")]
    pub test_reruns: bool,
    /// Runs in a row with nothing changed between, the first included, before the note.
    #[serde(default = "default_test_rerun_note_at")]
    pub test_rerun_note_at: u32,
    /// Runs before the model is asked for a different plan.
    #[serde(default = "default_test_rerun_replan_at")]
    pub test_rerun_replan_at: u32,
    /// Whether steps in a row that bring nothing new get a note asking where the turn
    /// stands, then a request for a different plan: no project file changed, nothing new
    /// read or shown, nothing that failed passed. Notes only: past the re-plan the turn
    /// is a stall candidate, which only a judge may end.
    #[serde(default = "yes")]
    pub no_progress: bool,
    /// Steps in a row with nothing new before the note, and between notes after the
    /// re-plan.
    #[serde(default = "default_no_progress_note_at")]
    pub no_progress_note_at: u32,
    /// Steps before the model is asked for a different plan.
    #[serde(default = "default_no_progress_replan_at")]
    pub no_progress_replan_at: u32,
    /// Whether a turn that a rule marks a stall candidate (still no progress after the
    /// re-plan, or a cycle asked twice for a different plan) is read by a judge model: it
    /// ends the turn only when confident it is stuck, and otherwise its advice goes to
    /// the model as a note and the next look waits longer.
    #[serde(default = "yes")]
    pub loop_judge: bool,
    /// Judge calls a turn may make at most.
    #[serde(default = "default_loop_judge_calls")]
    pub loop_judge_calls: u32,
    /// Whether the model's text repeating one piece over and over, outside code, tables
    /// and lists, is cut short and the step taken again with a note.
    #[serde(default = "yes")]
    pub text_loops: bool,
    /// Times a 50-character piece of text comes back close together before the cut.
    #[serde(default = "default_text_loop_repeats")]
    pub text_loop_repeats: u32,
    /// Whether a goal pauses for the user once its rounds stop changing anything: the
    /// same project files and the same test results after each.
    #[serde(default = "yes")]
    pub goal_stall: bool,
    /// Rounds in a row that changed nothing before the goal pauses.
    #[serde(default = "default_goal_stall_rounds")]
    pub goal_stall_rounds: u32,
    /// Whether the files a command changes in the project are recorded, with a diff when
    /// the text before is known, so the agent and clients can ask what changed. Off, no
    /// command is watched and the chat's list of changed files stays empty too.
    #[serde(default = "yes")]
    pub record_changes: bool,
    /// Whether old tool results are sent as short stubs that say how to get them back,
    /// instead of word for word on every request. The full text stays stored.
    #[serde(default = "yes")]
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
    #[serde(default = "yes")]
    pub compact_tail: bool,
    /// The most a kept tail holds, in estimated tokens; never more than a fifth of the
    /// model's input limit, so a compaction always frees most of it.
    #[serde(default = "default_compact_tail_tokens")]
    pub compact_tail_tokens: u64,
    /// Whether the core compacts a context on its own as it fills up. Off, a chat whose
    /// context is full stops with an error until the user compacts it by hand.
    #[serde(default = "yes")]
    pub auto_compact: bool,
    /// The share of the model's input limit, in percent, at which a running turn compacts
    /// the context. A value outside 60 to 80 is brought to the nearest bound on load.
    #[serde(default = "default_auto_compact_percent")]
    pub auto_compact_percent: u64,
    /// Whether the context is also compacted between turns once it passes 60% of the input
    /// limit, so the summary is written in the pause and not in the middle of a turn. Off
    /// until the evals show the cached share and the wall time do not get worse.
    #[serde(default = "yes")]
    pub compact_early: bool,
    /// Whether the output of a command whose program is known (cargo, pytest, npm, git
    /// transfers) is sent with passing tests, progress and repeats left out, and a note
    /// on how to read it whole. The full output stays stored.
    #[serde(default = "yes")]
    pub shape_output: bool,
    /// Whether a failed run of `cargo test`, `pytest` or jest through `npm test` gets one
    /// line naming the command that re-runs only the failing tests, when the command line
    /// and the output leave no doubt about it.
    #[serde(default = "yes")]
    pub rerun_hint: bool,
    /// Whether the model is offered `project_graph`: a map of the project's files and
    /// folders by name. The name is kept from the code graph it replaced.
    #[serde(default = "yes")]
    pub project_graph: bool,
    /// Whether the core writes down what ended a problem in a saved project (a repeated
    /// error that then passed, a verification that passed) for the `recall` tool. Off
    /// until the evals show recalled notes help.
    #[serde(default = "yes")]
    pub episodes: bool,
    /// Whether a call that ends a repeated failure asks the model for a one-line lesson
    /// at the end of its answer, stored with the episode.
    #[serde(default = "yes")]
    pub episode_lesson: bool,
    /// The kill switch of teams. On, a chat in TEAM mode gets the plan form of `tasks`
    /// (tasks that depend on each other, and `run`, which has the core start the team).
    /// Off, TEAM mode has the board like any other mode and no team runs.
    #[serde(default = "yes")]
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
    /// Older configs set a team's budget; teams have none now, so these are read and dropped.
    #[serde(default, skip_serializing)]
    team_max_steps: Option<u64>,
    #[serde(default, skip_serializing)]
    team_max_tokens: Option<u64>,
    #[serde(default, skip_serializing)]
    team_max_minutes: Option<u64>,
    /// The longest one gate command may run before it is killed and counts as failed.
    #[serde(default = "default_team_gate_seconds")]
    pub team_gate_seconds: u64,
    /// The most gate commands one team may run; past it a gated task waits for the lead.
    #[serde(default = "default_team_max_gate_runs")]
    pub team_max_gate_runs: u32,
    /// How long members at work may take to hand over once their team winds down (it
    /// was stopped, went quiet, or its provider keeps failing); then it ends.
    #[serde(default = "default_team_wrap_seconds")]
    pub team_wrap_seconds: u64,
    /// Whether a fresh reviewer checks each team task that changed files, with one round
    /// of fixes by its author, and a critic checks the results against the request at
    /// the end. Off, a task without a gate waits for the lead and the team ends when
    /// its last task is done.
    #[serde(default = "yes")]
    pub team_review: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: None,
            max_output_tokens: None,
            max_agents_at_work: default_max_agents_at_work(),
            max_goal_rounds: None,
            model_timeout_seconds: default_model_timeout_seconds(),
            max_attempts: default_max_attempts(),
            retry_unreachable_seconds: default_retry_unreachable_seconds(),
            resume_within_seconds: default_resume_within_seconds(),
            wait_for_provider: true,
            shrink_context: true,
            failure_ladder: true,
            read_many_files: true,
            parallel_reads: true,
            stable_prefix: true,
            failure_note_at: default_failure_note_at(),
            failure_replan_at: default_failure_replan_at(),
            failure_stop_at: default_failure_stop_at(),
            loop_cycles: true,
            cycle_note_at: default_cycle_note_at(),
            cycle_replan_at: default_cycle_replan_at(),
            test_reruns: true,
            test_rerun_note_at: default_test_rerun_note_at(),
            test_rerun_replan_at: default_test_rerun_replan_at(),
            no_progress: true,
            no_progress_note_at: default_no_progress_note_at(),
            no_progress_replan_at: default_no_progress_replan_at(),
            loop_judge: true,
            loop_judge_calls: default_loop_judge_calls(),
            text_loops: true,
            text_loop_repeats: default_text_loop_repeats(),
            goal_stall: true,
            goal_stall_rounds: default_goal_stall_rounds(),
            record_changes: true,
            stub_results: true,
            stub_keep_steps: default_stub_keep_steps(),
            stub_min_tokens: default_stub_min_tokens(),
            compact_tail: true,
            compact_tail_tokens: default_compact_tail_tokens(),
            auto_compact: true,
            auto_compact_percent: default_auto_compact_percent(),
            compact_early: true,
            shape_output: true,
            rerun_hint: true,
            project_graph: true,
            episodes: true,
            episode_lesson: true,
            plan_graph: true,
            task_packets: false,
            member_worktrees: false,
            peer_messages: false,
            procedures: false,
            team_at_once: default_team_at_once(),
            team_max_steps: None,
            team_max_tokens: None,
            team_max_minutes: None,
            team_gate_seconds: default_team_gate_seconds(),
            team_max_gate_runs: default_team_max_gate_runs(),
            team_wrap_seconds: default_team_wrap_seconds(),
            team_review: true,
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

/// Three rounds, as the same call in a row always had; a cycle of four calls then
/// spans twelve.
fn default_cycle_note_at() -> u32 {
    3
}

fn default_cycle_replan_at() -> u32 {
    5
}

/// The first run again with nothing changed already tells nothing new.
fn default_test_rerun_note_at() -> u32 {
    2
}

fn default_test_rerun_replan_at() -> u32 {
    4
}

/// A step that only rereads is common; a dozen in a row is not research any more.
fn default_no_progress_note_at() -> u32 {
    12
}

fn default_no_progress_replan_at() -> u32 {
    30
}

/// Steps in a row with nothing new have to span a few at least, and stay well short of a
/// turn that burns itself out.
pub const MIN_NO_PROGRESS_STEPS: u32 = 3;
pub const MAX_NO_PROGRESS_STEPS: u32 = 500;

/// A judge per stall candidate, the look after a redirect waiting longer each time:
/// three reach far into a turn.
fn default_loop_judge_calls() -> u32 {
    3
}

pub const MIN_LOOP_JUDGE_CALLS: u32 = 1;
pub const MAX_LOOP_JUDGE_CALLS: u32 = 10;

/// Gemini CLI's count: ten times one piece of text within a few hundred characters is
/// no longer prose.
fn default_text_loop_repeats() -> u32 {
    10
}

/// Fewer and a refrain or a list in prose would be cut.
pub const MIN_TEXT_LOOP_REPEATS: u32 = 5;
pub const MAX_TEXT_LOOP_REPEATS: u32 = 100;

/// One round that changed nothing can be a look around; three is a goal going nowhere.
fn default_goal_stall_rounds() -> u32 {
    3
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
fn default_auto_compact_percent() -> u64 {
    AUTO_COMPACT_PERCENT
}

impl Limits {
    /// Brings `auto_compact_percent` within its bounds.
    pub fn clamp_auto_compact(&mut self) {
        self.auto_compact_percent = clamp_auto_compact_percent(self.auto_compact_percent);
    }
}

/// The nearest share within the bounds of `limits.auto_compact_percent`.
pub fn clamp_auto_compact_percent(percent: u64) -> u64 {
    percent.clamp(MIN_AUTO_COMPACT_PERCENT, MAX_AUTO_COMPACT_PERCENT)
}

pub const MIN_COMPACT_TAIL_TOKENS: u64 = 2_000;
pub const MAX_COMPACT_TAIL_TOKENS: u64 = 64_000;

fn default_compact_tail_tokens() -> u64 {
    16_000
}

/// Members of a team at work at once. The floor keeps a plan able to finish one task.
pub const MIN_TEAM_AT_ONCE: usize = 1;
pub const MAX_TEAM_AT_ONCE: usize = 8;
/// A gate is a check such as a build and its tests; the top is a command's own longest.
pub const MIN_TEAM_GATE_SECONDS: u64 = 10;
pub const MAX_TEAM_GATE_SECONDS: u64 = 600;
pub const MIN_TEAM_MAX_GATE_RUNS: u32 = 1;
pub const MAX_TEAM_MAX_GATE_RUNS: u32 = 1_000;
pub const MIN_TEAM_WRAP_SECONDS: u64 = 10;
pub const MAX_TEAM_WRAP_SECONDS: u64 = 1_800;

fn default_team_at_once() -> usize {
    3
}

fn default_team_gate_seconds() -> u64 {
    300
}

/// Three minutes: a member's next step or two, and its footer.
fn default_team_wrap_seconds() -> u64 {
    180
}

/// Two gate runs a task (its first and a rework) for a plan of 64 tasks, the most open.
fn default_team_max_gate_runs() -> u32 {
    128
}

/// A role name is an agent's folder name.
const MAX_ROLE_NAME: usize = 64;

/// How apps share the core. Off, every app starts its own core as before.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSection {
    /// The first app owns the core and serves it on a private socket; later apps attach.
    #[serde(default)]
    pub shared: bool,
}

impl CoreSection {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

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
    /// The tools a member of a role keeps, by role. A list only takes tools away from
    /// what every member may have; a role without one keeps them all.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, Vec<String>>,
    /// The role that reviews a task that changed files; `code-reviewer` when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    /// The role that checks the results against the request at the end; `code-reviewer`
    /// when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critic: Option<String>,
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
        for tool in self.tools.values().flatten() {
            anyhow::ensure!(
                !tool.is_empty()
                    && tool.len() <= MAX_ROLE_NAME
                    && tool.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "team.tools names tools, such as read_file"
            );
        }
        let roles = self.models.keys().chain(self.tools.keys());
        for role in roles.chain(&self.reviewer).chain(&self.critic) {
            anyhow::ensure!(
                !role.is_empty()
                    && role.len() <= MAX_ROLE_NAME
                    && role
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "team.models and team.tools keys, team.reviewer and team.critic must be role \
                 names of up to {MAX_ROLE_NAME} of a-z, 0-9, - and _"
            );
        }
        Ok(())
    }

    /// The role a review of a task is given to.
    pub fn reviewer_role(&self) -> &str {
        self.reviewer.as_deref().unwrap_or(DEFAULT_CHECK_ROLE)
    }

    /// The role the completeness check at the end of a team is given to.
    pub fn critic_role(&self) -> &str {
        self.critic.as_deref().unwrap_or(DEFAULT_CHECK_ROLE)
    }
}

/// The shipped role that reviews and checks when the settings name none.
const DEFAULT_CHECK_ROLE: &str = "code-reviewer";

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
