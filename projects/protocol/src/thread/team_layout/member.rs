//! What a member does now, from its task's status and its own events, and the words a
//! node says for it.

use crate::thread::{
    AgentState, GoalCheck, Subagent, Task, TaskStatus, TeamMember, WaitsOn, WorkKind, find_task,
};

/// What a member does now. The working ones follow the member's own events; the rest
/// its task's status, as the core holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberState {
    Thinking,
    /// Runs a tool, by its name.
    Tool(String),
    Writing,
    /// Given its task; its turn has not started.
    Given,
    /// Handed in; `for_user` when the gate is the user's confirmation.
    Review {
        for_user: bool,
    },
    Blocked,
    Done,
    Failed,
    /// Its turn stopped and its task went back: the team ended, or the task was taken
    /// back and given out again.
    Stopped,
}

impl MemberState {
    /// What `member` does: `task` is its task in the plan, `agent` the chat's agent of its
    /// thread, `ended` whether the team ended.
    pub fn of(
        member: &TeamMember,
        task: Option<&Task>,
        agent: Option<&Subagent>,
        ended: bool,
    ) -> Self {
        // A task's own events come more often than the team's; they lead while the task
        // is still this member's.
        let task_is_mine = task
            .and_then(|task| task.owner.as_ref())
            .is_some_and(|owner| owner.thread_id == member.thread_id);
        let status = match task {
            Some(task) if task_is_mine => task.status,
            // Given to someone else since: a failed member stays failed; the rest stopped.
            Some(_) if member.status != TaskStatus::Failed => TaskStatus::Open,
            _ => member.status,
        };
        match status {
            // The end stopped every member at work or waiting; a hand-in stays in review.
            _ if ended && (status.at_work() || status == TaskStatus::Blocked) => Self::Stopped,
            TaskStatus::Open => Self::Stopped,
            TaskStatus::Claimed => Self::Given,
            TaskStatus::Running => match agent.map(|agent| (&agent.state, &agent.running)) {
                Some((AgentState::Writing, _)) => Self::Writing,
                Some((AgentState::Tool(name), running)) => Self::Tool(
                    running
                        .last()
                        .map_or_else(|| name.clone(), |(_, name)| name.clone()),
                ),
                _ => Self::Thinking,
            },
            TaskStatus::Blocked => Self::Blocked,
            TaskStatus::Review => Self::Review {
                for_user: task.is_some_and(|task| task.gate == Some(GoalCheck::UserConfirmation)),
            },
            TaskStatus::Done => Self::Done,
            TaskStatus::Failed => Self::Failed,
        }
    }

    /// Thinking, running a tool, writing or about to start.
    pub fn working(&self) -> bool {
        matches!(
            self,
            Self::Thinking | Self::Tool(_) | Self::Writing | Self::Given
        )
    }

    /// Stays a full node: at work, handed in, blocked or failed.
    pub fn active(&self) -> bool {
        !matches!(self, Self::Done | Self::Stopped)
    }

    /// The working row's kind whose moving mark a client draws for it; the others draw
    /// the glyph of `status`.
    pub fn work_kind(&self) -> Option<WorkKind> {
        match self {
            Self::Thinking => Some(WorkKind::Thinking),
            Self::Tool(_) => Some(WorkKind::Steps),
            Self::Writing => Some(WorkKind::Writing),
            Self::Review { for_user: true } => Some(WorkKind::WaitingForUser),
            _ => None,
        }
    }

    /// The task status whose glyph and tone stand for it.
    pub fn status(&self) -> TaskStatus {
        match self {
            Self::Thinking | Self::Tool(_) | Self::Writing => TaskStatus::Running,
            Self::Given => TaskStatus::Claimed,
            Self::Review { .. } => TaskStatus::Review,
            Self::Blocked => TaskStatus::Blocked,
            Self::Done => TaskStatus::Done,
            Self::Failed => TaskStatus::Failed,
            Self::Stopped => TaskStatus::Open,
        }
    }

    /// One or two words for a narrow row: "in work", "rework", "blocked".
    pub fn word(&self, task: Option<&Task>) -> &'static str {
        match self {
            _ if self.working() && task.is_some_and(|task| task.reworks > 0) => "rework",
            _ if self.working() => "in work",
            Self::Review { for_user: true } => "confirm",
            Self::Review { .. } => "in review",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Failed => "failed",
            _ => "stopped",
        }
    }

    /// What it does, in a line: "running a command", "in review · the lead decides",
    /// "blocked · asks the lead: which clock to mock?". `seed` picks among a tool's
    /// phrases, stable for one member.
    pub fn line(&self, task: Option<&Task>, tasks: &[Task], ended: bool, seed: &str) -> String {
        let note = task.map(|task| task.note.trim()).unwrap_or_default();
        let with_note = |head: String| match note.is_empty() {
            true => head,
            false => format!("{head}: {note}"),
        };
        match self {
            Self::Thinking => "thinking".into(),
            Self::Tool(name) => lower_first(crate::tools::phrase(name, seed)),
            Self::Writing => "writing".into(),
            Self::Given => "given its task, starting".into(),
            Self::Review { for_user: true } => {
                "handed in \u{00B7} waits for your confirmation".into()
            }
            Self::Review { .. } => match task.map(|task| (&task.waits_on, &task.gate)) {
                Some((Some(WaitsOn::Review(_)), _)) => {
                    "in review \u{00B7} a fresh reviewer checks it".into()
                }
                Some((_, Some(GoalCheck::Command { command }))) => {
                    format!("in review \u{00B7} the gate runs {command}")
                }
                _ => "in review \u{00B7} the lead decides".into(),
            },
            Self::Blocked => {
                let on = match task.and_then(|task| task.waits_on.as_ref()) {
                    Some(WaitsOn::Lead) => "asks the lead".to_owned(),
                    Some(WaitsOn::Task(id)) => match find_task(tasks, id) {
                        Some((number, _)) => format!("waits on #{number}"),
                        None => "waits on a task".to_owned(),
                    },
                    Some(WaitsOn::Message(_)) => "waits for an answer".to_owned(),
                    Some(WaitsOn::Review(_)) | None => String::new(),
                };
                match on.is_empty() {
                    true => with_note("blocked".into()),
                    false => with_note(format!("blocked \u{00B7} {on}")),
                }
            }
            Self::Done => "done".into(),
            Self::Failed => with_note("failed".into()),
            Self::Stopped if ended => "stopped with the team".into(),
            Self::Stopped => "its task was given back".into(),
        }
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::TaskOwner;

    fn member(status: TaskStatus) -> TeamMember {
        TeamMember {
            thread_id: "m1".into(),
            name: "Ember".into(),
            role: None,
            model: None,
            task_id: "t1".into(),
            status,
        }
    }

    fn task(status: TaskStatus, owner: &str) -> Task {
        Task {
            task_id: "t1".into(),
            status,
            done: status == TaskStatus::Done,
            owner: Some(TaskOwner {
                thread_id: owner.into(),
                name: owner.into(),
            }),
            ..Task::default()
        }
    }

    fn agent(state: AgentState) -> Subagent {
        Subagent {
            thread_id: "m1".into(),
            name: "Ember".into(),
            role: "general".into(),
            model: None,
            call_id: "team:t1".into(),
            task: String::new(),
            task_id: Some("t1".into()),
            state,
            running: Vec::new(),
            started_at: 0,
            ended_at: None,
        }
    }

    #[test]
    fn a_member_follows_its_own_events_while_its_task_runs() {
        let running = task(TaskStatus::Running, "m1");
        let doing = |state| {
            MemberState::of(
                &member(TaskStatus::Running),
                Some(&running),
                Some(&agent(state)),
                false,
            )
        };
        assert_eq!(doing(AgentState::Thinking), MemberState::Thinking);
        assert_eq!(doing(AgentState::Writing), MemberState::Writing);
        assert_eq!(
            doing(AgentState::Tool("run_command".into())),
            MemberState::Tool("run_command".into())
        );
        assert_eq!(
            MemberState::of(&member(TaskStatus::Running), Some(&running), None, false),
            MemberState::Thinking
        );
    }

    #[test]
    fn an_ended_team_or_a_task_given_again_stops_a_member() {
        let running = task(TaskStatus::Running, "m1");
        let member_running = member(TaskStatus::Running);
        assert_eq!(
            MemberState::of(&member_running, Some(&running), None, true),
            MemberState::Stopped
        );
        let blocked = task(TaskStatus::Blocked, "m1");
        assert_eq!(
            MemberState::of(&member(TaskStatus::Blocked), Some(&blocked), None, true),
            MemberState::Stopped
        );
        // The task moved on to another member.
        let theirs = task(TaskStatus::Running, "m2");
        assert_eq!(
            MemberState::of(&member_running, Some(&theirs), None, false),
            MemberState::Stopped
        );
        // A failed member stays failed though its task was given again.
        assert_eq!(
            MemberState::of(&member(TaskStatus::Failed), Some(&theirs), None, false),
            MemberState::Failed
        );
        // The task's own status leads the team's.
        let review = Task {
            gate: Some(GoalCheck::UserConfirmation),
            ..task(TaskStatus::Review, "m1")
        };
        let doing = MemberState::of(&member_running, Some(&review), None, false);
        assert_eq!(doing, MemberState::Review { for_user: true });
        assert_eq!(doing.work_kind(), Some(WorkKind::WaitingForUser));
    }

    #[test]
    fn a_blocked_member_says_what_it_waits_on() {
        let mut blocked = task(TaskStatus::Blocked, "m1");
        blocked.waits_on = Some(WaitsOn::Lead);
        blocked.note = " which clock? ".into();
        let line = MemberState::Blocked.line(Some(&blocked), &[], false, "m1");
        assert_eq!(line, "blocked \u{00B7} asks the lead: which clock?");
        assert_eq!(
            MemberState::Tool("run_command".into()).line(None, &[], false, "m1"),
            lower_first(crate::tools::phrase("run_command", "m1"))
        );
        assert_eq!(
            MemberState::Stopped.line(None, &[], true, "m1"),
            "stopped with the team"
        );
    }
}
