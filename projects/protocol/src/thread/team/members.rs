//! A team's members as rows: who, as what, on which model, on which task, at what cost,
//! and how its task stands.

use super::{Team, plan::find_task, roster::role_name};
use crate::thread::{Task, TaskStatus, TeamMember};

/// One member as a row shows it. The task's words are model text: plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    /// Its thread, which a click opens.
    pub thread_id: String,
    pub name: String,
    pub role: String,
    /// The model without its provider: `gpt-5.5`.
    pub model: Option<String>,
    /// "#3 Remove expired entries in get()", when the plan holds its task.
    pub task: Option<String>,
    /// "31 steps · 204k tokens" of its task, once it spent anything.
    pub spend: Option<String>,
    pub status: TaskStatus,
}

impl MemberRow {
    /// "31 steps · 204k tokens · backend-engineer · gpt-5.5": what is known of it after
    /// its name, its spend first as what a narrow row keeps longest.
    pub fn detail(&self) -> String {
        [self.spend.as_ref(), Some(&self.role), self.model.as_ref()]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" \u{00B7} ")
    }
}

impl MemberRow {
    /// "#3 Remove expired entries · 31 steps · 204k tokens · backend-engineer · gpt-5.5":
    /// its task, then the rest of its detail, as the line under its name.
    pub fn line(&self) -> String {
        match &self.task {
            Some(task) => format!("{task} \u{00B7} {}", self.detail()),
            None => self.detail(),
        }
    }
}

impl Team {
    /// Its members as rows, in the team's order, with their tasks found in `tasks`, the
    /// plan as the chat holds it.
    pub fn member_rows(&self, tasks: &[Task]) -> Vec<MemberRow> {
        self.members
            .iter()
            .map(|member| {
                let task = find_task(tasks, &member.task_id);
                let model = member
                    .model
                    .as_deref()
                    .or(task.and_then(|(_, task)| task.model.as_deref()));
                MemberRow {
                    thread_id: member.thread_id.clone(),
                    name: member.name.clone(),
                    role: role_name(member.role.as_deref()).to_owned(),
                    model: model.map(short_model),
                    task: task
                        .map(|(number, task)| task.numbered(number))
                        .or_else(|| member.summary()),
                    spend: task.and_then(|(_, task)| task.spend()),
                    status: member.status,
                }
            })
            .collect()
    }
}

impl TeamMember {
    /// What it does when the plan does not say: the core's summary, if it wrote one.
    pub fn summary(&self) -> Option<String> {
        (!self.summary.is_empty()).then(|| self.summary.clone())
    }
}

/// `gpt-5.5` of `chatgpt/gpt-5.5`.
pub fn short_model(model: &str) -> String {
    model
        .split_once('/')
        .map_or(model, |(_, name)| name)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{TeamStatus, TeamUsed};

    #[test]
    fn a_member_row_names_its_task_model_and_spend() {
        let tasks = [
            Task {
                task_id: "t1".into(),
                description: "Plan  the layout".into(),
                ..Task::default()
            },
            Task {
                task_id: "t2".into(),
                description: "Remove expired entries".into(),
                status: TaskStatus::Running,
                model: Some("chatgpt/gpt-5.5".into()),
                steps: 31,
                tokens: 204_000,
                ..Task::default()
            },
        ];
        let member = |thread: &str, task: &str, role: Option<&str>| TeamMember {
            thread_id: thread.into(),
            name: thread.to_uppercase(),
            role: role.map(Into::into),
            model: None,
            task_id: task.into(),
            summary: String::new(),
            status: TaskStatus::Running,
        };
        let team = Team {
            thread_id: "lead".into(),
            status: TeamStatus::Running,
            at_once: 2,
            used: TeamUsed::default(),
            members: vec![
                member("m2", "t2", Some("backend-engineer")),
                member("m9", "gone", None),
            ],
            started_at: None,
            ended_at: None,
            winding_since: None,
            goal: String::new(),
        };
        let rows = team.member_rows(&tasks);
        assert_eq!(rows[0].task.as_deref(), Some("#2 Remove expired entries"));
        assert_eq!(
            rows[0].detail(),
            "31 steps \u{00B7} 204k tokens \u{00B7} backend-engineer \u{00B7} gpt-5.5"
        );
        assert_eq!(
            (rows[1].task.as_deref(), rows[1].detail()),
            (None, "general".into())
        );
    }

    #[test]
    fn a_member_whose_task_the_plan_lacks_shows_the_cores_summary() {
        let team = Team {
            thread_id: "lead".into(),
            status: TeamStatus::Running,
            at_once: 1,
            used: TeamUsed::default(),
            members: vec![TeamMember {
                thread_id: "m1".into(),
                name: "Ember-0c".into(),
                role: Some("code-reviewer".into()),
                model: None,
                task_id: "t9".into(),
                summary: "review of #77aa01bc".into(),
                status: TaskStatus::Running,
            }],
            started_at: None,
            ended_at: None,
            winding_since: None,
            goal: String::new(),
        };
        let rows = team.member_rows(&[]);
        assert_eq!(rows[0].task.as_deref(), Some("review of #77aa01bc"));
        assert_eq!(rows[0].line(), "review of #77aa01bc \u{00B7} code-reviewer");
    }
}
