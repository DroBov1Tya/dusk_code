//! The phase an agent reports it is in.

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    Planning,
    Executing,
    Reviewing,
    Done,
    Paused,
}

impl AgentPhase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::Reviewing => "reviewing",
            Self::Done => "done",
            Self::Paused => "paused",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "planning" => Self::Planning,
            "executing" => Self::Executing,
            "reviewing" => Self::Reviewing,
            "done" => Self::Done,
            "paused" => Self::Paused,
            other => bail!("unknown agent phase {other}"),
        })
    }

    /// Where work may go from here; `None` is an agent with no phase yet.
    pub fn next(from: Option<Self>) -> &'static [Self] {
        use AgentPhase::*;
        match from {
            None | Some(Done) => &[Planning, Executing],
            Some(Planning) => &[Executing, Paused],
            Some(Executing) => &[Reviewing, Paused],
            Some(Reviewing) => &[Executing, Done, Paused],
            Some(Paused) => &[Planning, Executing, Reviewing],
        }
    }

    /// Still at work: a stop or a crash in this phase leaves the agent paused.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Planning | Self::Executing | Self::Reviewing)
    }
}
