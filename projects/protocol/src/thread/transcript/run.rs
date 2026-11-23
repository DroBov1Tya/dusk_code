//! A run the chat started: its team set to work on the plan, or subagents asked in one
//! step. The chat keeps the calls that started it as one item where it began, and every
//! client draws that item as one card: a title, "Team · 5 agents · 1m 12s", and a mark a
//! task or agent.

use crate::{
    model::ToolCall,
    thread::{GoalCheck, Task, TaskStatus, duration_words},
};

use super::{AgentState, Tool, ToolState};

/// The lead's plan tool; its `run` action sets the team to work.
pub const PLAN_TOOL: &str = "tasks";
/// The tool that asks a subagent.
pub const ASK_AGENT_TOOL: &str = "ask_agent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Team,
    Subagents,
}

/// The calls that started a run, oldest first: one `tasks` run for a team, every
/// `ask_agent` of one step for subagents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub kind: RunKind,
    pub tools: Vec<Tool>,
}

impl Run {
    /// The kind of run `call` starts, if it starts one.
    pub fn started_by(call: &ToolCall) -> Option<RunKind> {
        match call.name.as_str() {
            ASK_AGENT_TOOL => Some(RunKind::Subagents),
            PLAN_TOOL
                if call
                    .arguments
                    .get("action")
                    .and_then(|action| action.as_str())
                    == Some("run") =>
            {
                Some(RunKind::Team)
            }
            _ => None,
        }
    }

    pub fn tool_mut(&mut self, call_id: &str) -> Option<&mut Tool> {
        self.tools.iter_mut().find(|tool| tool.call_id == call_id)
    }
}

/// One task's or agent's mark on a card, drawn as the plan's squares are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMark {
    Done,
    Running,
    /// Given out and not yet running, or in review.
    Waiting,
    /// Its review waits for the user.
    Confirm,
    Open,
    Blocked,
    Failed,
}

impl RunMark {
    pub fn of_task(task: &Task) -> Self {
        match task.status {
            TaskStatus::Done => Self::Done,
            TaskStatus::Running => Self::Running,
            TaskStatus::Review if task.gate == Some(GoalCheck::UserConfirmation) => Self::Confirm,
            TaskStatus::Claimed | TaskStatus::Review => Self::Waiting,
            TaskStatus::Open => Self::Open,
            TaskStatus::Blocked => Self::Blocked,
            TaskStatus::Failed => Self::Failed,
        }
    }

    /// A subagent by what it does now; one the chat never heard start (read from history)
    /// by how its call ended.
    pub fn of_agent(state: Option<&AgentState>, call: ToolState) -> Self {
        match (state, call) {
            (_, ToolState::Failed) | (Some(AgentState::Failed), _) => Self::Failed,
            (Some(AgentState::Done), _) => Self::Done,
            (Some(_), _) => Self::Running,
            (None, ToolState::Done) => Self::Done,
            (None, ToolState::Running) => Self::Open,
        }
    }

    /// The mark as a character, for a client without shapes: told apart without colour.
    pub fn char(self) -> char {
        match self {
            Self::Done => '\u{25A0}',
            Self::Running => '\u{25A3}',
            Self::Failed => '\u{25A8}',
            _ => '\u{25A1}',
        }
    }
}

/// What a card shows of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCard {
    pub kind: RunKind,
    pub title: String,
    pub agents: usize,
    /// How long it ran, until now while it runs; none when not known.
    pub elapsed_ms: Option<i64>,
    pub running: bool,
    /// A team run before the chat's last one: what it did is no longer known.
    pub earlier: bool,
    pub marks: Vec<RunMark>,
}

impl RunCard {
    /// "Team · 5 agents · 1m 12s", or "Team · ended" for an earlier run.
    pub fn line(&self) -> String {
        let kind = match self.kind {
            RunKind::Team => "Team",
            RunKind::Subagents => "Subagents",
        };
        if self.earlier {
            return format!("{kind} \u{00B7} ended");
        }
        let mut line = kind.to_owned();
        match self.agents {
            // A team not yet heard from has no members to count.
            0 => {}
            1 => line.push_str(" \u{00B7} 1 agent"),
            count => line.push_str(&format!(" \u{00B7} {count} agents")),
        }
        if let Some(ms) = self.elapsed_ms {
            line.push_str(&format!(" \u{00B7} {}", duration_words(ms)));
        }
        line
    }

    /// The marks as text, for a client without shapes.
    pub fn marks_text(&self) -> String {
        self.marks.iter().map(|mark| mark.char()).collect()
    }
}

/// The marks of a plan's tasks, in the plan's order.
pub fn task_marks(tasks: &[Task]) -> Vec<RunMark> {
    tasks.iter().map(RunMark::of_task).collect()
}
