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
        }
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
