//! A turn: one agent run.

use serde::{Deserialize, Serialize};

use crate::model::{ProviderErrorKind, ProviderFailure, ProviderWaitKind};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub turn_id: String,
    pub thread_id: String,
    /// Sequence of the newest user message this turn answers.
    pub last_sequence: u64,
    pub response_message_id: Option<String>,
    pub status: TurnStatus,
    /// Unix timestamps in milliseconds.
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error_message: Option<String>,
    /// Set while the running turn waits; never on a turn that ended. Absent from older
    /// peers, which knew no waits.
    #[serde(default)]
    pub waiting: Option<TurnWait>,
    /// The model calls of the turn that answered so far. This and the two below are
    /// counted by the core; absent from older peers, which sent no spend.
    #[serde(default)]
    pub steps: u32,
    /// Prompt and completion tokens of every model call of the turn.
    #[serde(default)]
    pub tokens: u64,
    /// The summed cost of the turn's calls whose cost is known.
    #[serde(default)]
    pub cost_micro_usd: u64,
}

/// What a running turn waits for. Times are Unix timestamps in milliseconds.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnWait {
    /// A restart cut off a call that may have taken effect; the user decides.
    Decision { since: i64 },
    /// The provider cannot serve the turn now; it continues by itself at `wake_at`.
    /// `since` is when the first of the waits in a row began, which bounds them all.
    Provider {
        kind: ProviderWaitKind,
        since: i64,
        wake_at: i64,
    },
}

impl TurnWait {
    /// A wait for the provider; `None` for a kind no wait passes, such as a refused
    /// request or a key that was not accepted.
    pub fn provider(kind: ProviderErrorKind, since: i64, wake_at: i64) -> Option<Self> {
        let kind = ProviderWaitKind::try_from(kind).ok()?;
        Some(Self::Provider {
            kind,
            since,
            wake_at,
        })
    }

    /// The line every client shows: "Waiting for your decision", or "Rate limited;
    /// continues in 12m 5s" with the time left from `now` (Unix ms, the client's clock).
    /// Without `now`, as in a line that stays in the chat: "Rate limited; continues by
    /// itself".
    pub fn text(&self, now: Option<i64>) -> String {
        let (kind, wake_at) = match *self {
            Self::Decision { .. } => return "Waiting for your decision".into(),
            Self::Provider { kind, wake_at, .. } => (ProviderErrorKind::from(kind), wake_at),
        };
        let when = match now {
            None => "by itself".to_owned(),
            Some(now) if wake_at > now => format!("in {}", duration_words(wake_at - now)),
            Some(_) => "now".to_owned(),
        };
        format!("{}; continues {when}", kind.label())
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStatus {
    Running,
    Completed,
    /// Stopped by the user or a restart; a partial answer is kept when there was one.
    Interrupted,
    Failed,
}

impl TurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

/// Why a turn failed: what the one row at its end shows.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFailure {
    /// Safe to show: never a key, a header, a request body or a URL path.
    pub error: String,
    /// Set when the turn failed on a model call.
    pub provider: Option<ProviderFailure>,
}

impl TurnFailure {
    /// The row as every client says it: "Provider unreachable after 9 attempts: could not
    /// reach api.example.com: connection refused".
    pub fn text(&self) -> String {
        match self.provider {
            Some(ProviderFailure { kind, attempts }) if attempts > 1 => {
                format!("{} after {attempts} attempts: {}", kind.label(), self.error)
            }
            Some(ProviderFailure { kind, .. }) => format!("{}: {}", kind.label(), self.error),
            None => self.error.clone(),
        }
    }
}

/// How long a turn took, as every client says it: "8s", "2m 14s", "1h 3m".
pub fn duration_words(ms: i64) -> String {
    let seconds = (ms.max(0) + 500) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod wait_tests {
    use super::*;

    fn turn(waiting: Option<TurnWait>) -> Turn {
        Turn {
            turn_id: "turn".into(),
            thread_id: "thread".into(),
            last_sequence: 3,
            response_message_id: None,
            status: TurnStatus::Running,
            started_at: 1_000,
            finished_at: None,
            error_message: None,
            waiting,
            steps: 0,
            tokens: 0,
            cost_micro_usd: 0,
        }
    }

    #[test]
    fn a_turn_from_an_older_peer_does_not_wait() {
        let mut json = serde_json::to_value(turn(None)).unwrap();
        json.as_object_mut().unwrap().remove("waiting");
        let read: Turn = serde_json::from_value(json).unwrap();
        assert_eq!(read, turn(None));
    }

    #[test]
    fn both_waits_survive_the_wire() {
        let provider = TurnWait::provider(ProviderErrorKind::RateLimited, 5, 9).unwrap();
        for wait in [TurnWait::Decision { since: 5 }, provider] {
            let sent = turn(Some(wait));
            let json = serde_json::to_string(&sent).unwrap();
            assert_eq!(serde_json::from_str::<Turn>(&json).unwrap(), sent);
        }
    }

    #[test]
    fn only_a_failure_that_passes_by_itself_is_waited_out() {
        use ProviderErrorKind::*;
        for kind in [RateLimited, Quota, Unreachable, TimedOut, Overloaded] {
            let wait = TurnWait::provider(kind, 0, 1);
            assert!(
                matches!(wait, Some(TurnWait::Provider { kind: waited, .. })
                    if ProviderErrorKind::from(waited) == kind),
                "{kind:?}"
            );
        }
        for kind in [Auth, Refused, TooLong, Interrupted] {
            assert_eq!(TurnWait::provider(kind, 0, 1), None, "{kind:?}");
        }
    }

    #[test]
    fn a_wait_reads_as_one_line() {
        let wait = TurnWait::provider(ProviderErrorKind::RateLimited, 0, 725_000).unwrap();
        assert_eq!(wait.text(Some(0)), "Rate limited; continues in 12m 5s");
        assert_eq!(wait.text(Some(725_000)), "Rate limited; continues now");
        assert_eq!(wait.text(None), "Rate limited; continues by itself");
        let decision = TurnWait::Decision { since: 0 };
        assert_eq!(decision.text(Some(0)), "Waiting for your decision");
    }
}

#[cfg(test)]
mod duration_tests {
    use super::duration_words;

    #[test]
    fn durations_read_in_the_largest_two_units() {
        assert_eq!(duration_words(8_400), "8s");
        assert_eq!(duration_words(134_000), "2m 14s");
        assert_eq!(duration_words(3_780_000), "1h 3m");
        assert_eq!(duration_words(-5), "0s");
    }
}
