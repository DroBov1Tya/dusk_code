//! The smaller sections of the config: approvals, memory, limits, logging, appearance
//! and what threads record.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Ask is the default. Sandbox runs commands without asking, confined by the system to
/// the working folders; paths outside them and the screen are still asked. Full access
/// answers every request with yes. Arbiter hands each request to an agent that decides
/// in the user's place. The config folder stays closed either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    #[default]
    Ask,
    Sandbox,
    Arbiter,
    FullAccess,
}

impl ApprovalPolicy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Sandbox => "sandbox",
            Self::Arbiter => "arbiter",
            Self::FullAccess => "full access",
        }
    }

    pub(super) fn is_ask(&self) -> bool {
        *self == Self::Ask
    }
}

/// The memory of the user: whether the agent keeps it, and its size. Every request
/// carries it, so it is kept small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_memory_chars")]
    pub max_chars: usize,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chars: default_memory_chars(),
        }
    }
}

impl Memory {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Compact facts about the current project, kept separately from user memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkHistory {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_memory_chars")]
    pub max_chars: usize,
}

impl Default for WorkHistory {
    fn default() -> Self {
        Self {
            enabled: false,
            max_chars: default_memory_chars(),
        }
    }
}

impl WorkHistory {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_memory_chars() -> usize {
    2000
}

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
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: None,
            max_output_tokens: None,
            max_agents_at_work: default_max_agents_at_work(),
            max_goal_rounds: default_max_goal_rounds(),
        }
    }
}

fn default_max_agents_at_work() -> usize {
    8
}

fn default_max_goal_rounds() -> u32 {
    10
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

pub(super) fn yes() -> bool {
    true
}

pub(super) fn is_true(value: &bool) -> bool {
    *value
}

pub(super) fn is_zero(value: &u64) -> bool {
    *value == 0
}

pub(super) fn is_false(value: &bool) -> bool {
    !value
}
