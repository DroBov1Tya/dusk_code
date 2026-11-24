//! A chat's standing goal: a condition the agent works toward turn after turn, and what
//! the evaluator said of it after each turn. Also the check that ends a team's task.

use anyhow::{Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{duration_words, plural, short_count};

/// The longest condition `/goal` takes, in characters.
pub const MAX_GOAL_CHARS: usize = 4000;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    /// Each turn that ends is judged, and the next one starts until the goal is met.
    Active,
    /// Set but not going on by itself, after an error or a stop; the user's next message
    /// makes it active again.
    Paused,
}

/// One goal per chat, until it is met, found impossible or cleared.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub thread_id: String,
    /// The condition, as the user wrote it.
    pub text: String,
    pub status: GoalStatus,
    /// When it was set, or when the app started again with it: Unix ms.
    pub started_at: i64,
    /// Turns the evaluator judged since `started_at`.
    pub turns: u32,
    /// Tokens the chat and its agents spent since `started_at`.
    pub tokens: u64,
    /// The evaluator's newest reason.
    pub reason: Option<String>,
    /// Why it is paused.
    pub note: Option<String>,
}

/// What the evaluator said of the goal after a turn.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalVerdict {
    /// Another turn starts, with the reason as guidance.
    NotMet,
    /// The goal is cleared as achieved.
    Met,
    /// The goal is cleared as failed.
    Impossible,
}

/// One verdict as the chat shows it, with what the goal had cost when it was given.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEvaluation {
    pub verdict: GoalVerdict,
    pub reason: String,
    /// The turn judged, from 1 since the goal started.
    pub turn: u32,
    pub tokens: u64,
    pub elapsed_ms: i64,
}

impl GoalEvaluation {
    /// The line every client shows; the reason goes under it.
    pub fn title(&self) -> String {
        let cost = format!(
            "{} \u{00B7} {} \u{00B7} {} tokens",
            duration_words(self.elapsed_ms),
            plural(u64::from(self.turn), "turn"),
            short_count(self.tokens)
        );
        match self.verdict {
            GoalVerdict::NotMet => format!("Goal not met yet (turn {})", self.turn),
            GoalVerdict::Met => format!("Goal achieved \u{00B7} {cost}"),
            GoalVerdict::Impossible => format!("Goal failed \u{00B7} {cost}"),
        }
    }
}

impl Goal {
    /// The indicator: "◎ goal active · 4m 2s", from `now` in Unix ms.
    pub fn indicator(&self, now: i64) -> String {
        let state = match self.status {
            GoalStatus::Active => "active",
            GoalStatus::Paused => "paused",
        };
        format!(
            "\u{25CE} goal {state} \u{00B7} {}",
            duration_words(now - self.started_at)
        )
    }

    /// What `/goal` shows: the condition, how long and how much it took so far, and the
    /// evaluator's newest reason.
    pub fn status_text(&self, now: i64) -> String {
        let mut text = format!(
            "Goal ({}): {}\nElapsed {} \u{00B7} {} evaluated \u{00B7} {} tokens",
            match self.status {
                GoalStatus::Active => "active",
                GoalStatus::Paused => "paused",
            },
            self.text,
            duration_words(now - self.started_at),
            plural(u64::from(self.turns), "turn"),
            short_count(self.tokens)
        );
        if let Some(note) = &self.note {
            text.push_str(&format!("\nPaused: {note}"));
        }
        if let Some(reason) = &self.reason {
            text.push_str(&format!("\nLatest: {reason}"));
        }
        text
    }
}

/// What ends a team's task: a command that must exit 0, or the user's confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalCheck {
    Command { command: String },
    UserConfirmation,
}

impl GoalCheck {
    pub fn validate(&self) -> Result<()> {
        if let Self::Command { command } = self {
            ensure!(
                !command.trim().is_empty() && command.len() <= 16 * 1024,
                "the command needs 1 to 16384 bytes"
            );
        }
        Ok(())
    }
}

impl GoalStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            other => bail!("unknown goal status {other}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_reads_as_one_line() {
        let evaluation = GoalEvaluation {
            verdict: GoalVerdict::Met,
            reason: "tests pass".into(),
            turn: 3,
            tokens: 12_400,
            elapsed_ms: 242_000,
        };
        assert_eq!(
            evaluation.title(),
            "Goal achieved \u{00B7} 4m 2s \u{00B7} 3 turns \u{00B7} 12.4k tokens"
        );
    }
}
