//! How a team stands, in the words every client shows: its status with the reason it
//! ended, and the task the plan names as in work.

use super::{Team, TeamEnd, TeamStatus};
use crate::thread::{Task, current_task};

impl TeamEnd {
    /// Why it ended, as a client says it after "ended": "ran out of steps".
    pub fn reason(self) -> &'static str {
        match self {
            Self::Done => "every task is done",
            Self::Steps => "ran out of steps",
            Self::Tokens => "ran out of tokens",
            Self::Time => "ran out of time",
            Self::Stalled => "stalled, no task could start",
            Self::Stopped => "stopped",
            Self::Quiet => "paused after 30 member messages without you",
        }
    }
}

impl Team {
    pub fn ended(&self) -> Option<TeamEnd> {
        match self.status {
            TeamStatus::Running => None,
            TeamStatus::Ended(end) => Some(end),
        }
    }
}

/// The task a plan's folded line names, with its number in `tasks`: the first running,
/// else the first not over. None once the team that ran the plan ended: nothing is in
/// work then, whatever a task's last status says.
pub fn task_in_work<'a>(tasks: &'a [Task], team: Option<&Team>) -> Option<(usize, &'a Task)> {
    match team.is_some_and(|team| team.ended().is_some()) {
        true => None,
        false => current_task(tasks),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::TaskStatus;
    use crate::thread::{TeamBudget, TeamUsed};

    fn team(status: TeamStatus) -> Team {
        Team {
            thread_id: "lead".into(),
            status,
            budget: TeamBudget::new(3, 400, 4_000_000, 120).unwrap(),
            used: TeamUsed {
                minutes: 57,
                ..TeamUsed::default()
            },
            members: Vec::new(),
        }
    }

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            task_id: id.into(),
            description: format!("do  {id}\n now"),
            done: status == TaskStatus::Done,
            status,
            ..Task::default()
        }
    }

    #[test]
    fn a_team_says_why_it_ended() {
        assert_eq!(
            team(TeamStatus::Running).progress_note(),
            "running \u{00B7} 57 min"
        );
        let ended = |end| team(TeamStatus::Ended(end)).progress_note();
        assert_eq!(
            ended(TeamEnd::Steps),
            "ended \u{2014} ran out of steps after 57 min"
        );
        assert_eq!(
            ended(TeamEnd::Tokens),
            "ended \u{2014} ran out of tokens after 57 min"
        );
        assert_eq!(
            ended(TeamEnd::Time),
            "ended \u{2014} ran out of time after 57 min"
        );
        assert_eq!(
            ended(TeamEnd::Stalled),
            "ended \u{2014} stalled, no task could start after 57 min"
        );
        assert_eq!(ended(TeamEnd::Stopped), "stopped after 57 min");
        assert_eq!(ended(TeamEnd::Done), "done after 57 min");
    }

    #[test]
    fn an_ended_team_has_no_task_in_work() {
        let tasks = [
            task("a", TaskStatus::Done),
            task("b", TaskStatus::Blocked),
            task("c", TaskStatus::Running),
        ];
        let running = team(TeamStatus::Running);
        let (number, found) = task_in_work(&tasks, Some(&running)).unwrap();
        assert_eq!((number, found.task_id.as_str()), (3, "c"));
        assert_eq!(found.numbered(number), "#3 do c now");
        // Without a running one, the first not over; a plan with no team is the same.
        assert_eq!(task_in_work(&tasks[..2], None).unwrap().0, 2);
        let ended = team(TeamStatus::Ended(TeamEnd::Steps));
        assert!(task_in_work(&tasks, Some(&ended)).is_none());
    }
}
