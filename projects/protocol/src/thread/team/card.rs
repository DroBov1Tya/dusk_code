//! A task of the plan as a card: what it is, how it stands and why, what it waits on and
//! what it spent. Only the task's current state: the core keeps no history of it yet.

use super::{members::short_model, plan::find_task, roster::role_name};
use crate::thread::{GoalCheck, Task, TaskOwner, TaskStatus, WaitsOn, plural};

/// A task as its card shows it. Every text that came from the lead, a member or a
/// program is model text: plain text, never markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCard {
    pub task_id: String,
    /// "#4 Write tests for expiry".
    pub title: String,
    pub status: TaskStatus,
    /// "blocked · qa-engineer · gpt-5.5": its status word, role and model.
    pub head: String,
    /// The member that works on it or did, whose chat a click opens.
    pub owner: Option<TaskOwner>,
    /// What else is known of it, by name, in order; a field with nothing is left out.
    pub fields: Vec<(&'static str, String)>,
}

/// The card of the task `task_id` of `tasks`, the plan as the chat holds it.
pub fn task_card(tasks: &[Task], task_id: &str) -> Option<TaskCard> {
    let (number, task) = find_task(tasks, task_id)?;
    let role = role_name(task.role.as_deref());
    let mut head = vec![task.status.member_word().to_owned(), role.to_owned()];
    head.extend(task.model.as_deref().map(short_model));
    let named = |id: &str| match find_task(tasks, id) {
        Some((number, other)) => format!("{} ({})", other.numbered(number), other.status.name()),
        None => format!("a task no longer in the plan ({id})"),
    };
    let mut fields: Vec<(&'static str, String)> = Vec::new();
    if let Some(waits) = &task.waits_on {
        let on = match waits {
            WaitsOn::Lead => "the lead's decision".to_owned(),
            WaitsOn::Task(id) => named(id),
            WaitsOn::Message(_) => "an answer to a message".to_owned(),
        };
        fields.push(("Waits on", on));
    }
    let note = match task.status {
        TaskStatus::Blocked => "Why blocked",
        TaskStatus::Failed => "Why failed",
        _ => "Note",
    };
    fields.extend([
        (note, task.note.trim().to_owned()),
        ("Brief", task.brief.trim().to_owned()),
        ("Acceptance", task.acceptance.trim().to_owned()),
        ("Files", task.files.join(", ")),
    ]);
    fields.push((
        "Gate",
        match &task.gate {
            Some(GoalCheck::Command { command }) => command.clone(),
            Some(GoalCheck::UserConfirmation) => "your confirmation".to_owned(),
            None => "the lead decides".to_owned(),
        },
    ));
    let after: Vec<String> = task.depends_on.iter().map(|id| named(id)).collect();
    fields.push(("Depends on", after.join(", ")));
    fields.push(("Phase", task.phase.clone()));
    let mut work = vec![
        plural(task.attempts, "attempt"),
        plural(task.reworks, "rework"),
    ];
    work.extend(task.spend());
    if task.changes > 0 {
        work.push(format!("{} changed", plural(task.changes, "file")));
    }
    fields.push(("Work", work.join(" \u{00B7} ")));
    fields.retain(|(_, value)| !value.is_empty());
    Some(TaskCard {
        task_id: task.task_id.clone(),
        title: task.numbered(number),
        status: task.status,
        head: head.join(" \u{00B7} "),
        owner: task.owner.clone(),
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            task_id: id.into(),
            description: format!("do {id}"),
            done: status == TaskStatus::Done,
            status,
            ..Task::default()
        }
    }

    #[test]
    fn a_card_says_why_a_task_waits_and_what_it_spent() {
        let mut blocked = task("t2", TaskStatus::Blocked);
        blocked.role = Some("qa-engineer".into());
        blocked.model = Some("chatgpt/gpt-5.5".into());
        blocked.depends_on = vec!["t1".into(), "gone".into()];
        blocked.waits_on = Some(WaitsOn::Lead);
        blocked.note = " which clock to mock? ".into();
        blocked.files = vec!["tests/expiry.rs".into()];
        blocked.attempts = 1;
        blocked.steps = 12;
        blocked.tokens = 84_000;
        blocked.owner = Some(TaskOwner {
            thread_id: "m2".into(),
            name: "Slate-47".into(),
        });
        let tasks = [task("t1", TaskStatus::Done), blocked];

        let card = task_card(&tasks, "t2").unwrap();
        assert_eq!(card.title, "#2 do t2");
        assert_eq!(card.head, "blocked \u{00B7} qa-engineer \u{00B7} gpt-5.5");
        assert_eq!(
            card.owner.as_ref().map(|owner| owner.name.as_str()),
            Some("Slate-47")
        );
        let field = |name: &str| {
            card.fields
                .iter()
                .find(|(label, _)| *label == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(field("Waits on"), Some("the lead's decision"));
        assert_eq!(field("Why blocked"), Some("which clock to mock?"));
        assert_eq!(field("Files"), Some("tests/expiry.rs"));
        assert_eq!(field("Gate"), Some("the lead decides"));
        assert_eq!(
            field("Depends on"),
            Some("#1 do t1 (done), a task no longer in the plan (gone)")
        );
        assert_eq!(
            field("Work"),
            Some("1 attempt \u{00B7} 0 reworks \u{00B7} 12 steps \u{00B7} 84k tokens")
        );
        // Nothing is said of what the task does not have.
        assert_eq!(
            (field("Brief"), field("Acceptance"), field("Phase")),
            (None, None, None)
        );
        assert!(task_card(&tasks, "t9").is_none());
    }
}
