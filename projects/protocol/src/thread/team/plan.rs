//! A plan's tasks in the words, glyphs and tones every client shows.

use crate::thread::{Task, TaskDuty, TaskStatus, Tone, plural};

impl TaskStatus {
    /// Given out or running: a member works on it.
    pub fn at_work(self) -> bool {
        matches!(self, Self::Claimed | Self::Running)
    }

    /// Its glyph; the word is always beside it, so the colour is never the only sign.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Open => "\u{25CB}",
            Self::Claimed => "\u{25C9}",
            Self::Running => "\u{25CF}",
            Self::Blocked => "\u{25C6}",
            Self::Review => "\u{25C7}",
            Self::Done => "\u{2713}",
            Self::Failed => "\u{2715}",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Open => Tone::Neutral,
            Self::Claimed | Self::Review => Tone::Info,
            Self::Running => Tone::Accent,
            Self::Blocked => Tone::Warning,
            Self::Done => Tone::Success,
            Self::Failed => Tone::Danger,
        }
    }
}

impl Task {
    /// Its words on one line. Model text: a client draws it as plain text.
    pub fn title(&self) -> String {
        self.description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// "#3 Remove expired entries in get()", numbered as the plan's rows are.
    pub fn numbered(&self, number: usize) -> String {
        format!("#{number} {}", self.title())
    }

    /// Done, or given up by the lead: the next wave does not wait for it.
    pub fn finished(&self) -> bool {
        self.status == TaskStatus::Done || self.skipped
    }

    /// How its fresh review stands, as a chip beside it: "reviewing", "reviewed",
    /// "review: fix", "review failed", numbered from the second review on ("review 2 ·
    /// fix"). `None` for a task no reviewer took up.
    pub fn review_word(&self, tasks: &[Task]) -> Option<String> {
        let reviews: Vec<&Task> = tasks
            .iter()
            .filter(|row| {
                row.duty == TaskDuty::Review && row.review_of.as_deref() == Some(&self.task_id)
            })
            .collect();
        let last = reviews
            .iter()
            .max_by_key(|row| (row.created_at, &row.task_id))?;
        let verdict = last
            .result
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("Verdict:"))
            .map(|word| word.trim().trim_end_matches('.').to_ascii_lowercase());
        let word = match (last.status, verdict.as_deref()) {
            (TaskStatus::Done, Some("approve" | "approved")) => "reviewed",
            (TaskStatus::Done, Some("fix")) => "review: fix",
            (TaskStatus::Done, _) => "review: no verdict",
            (TaskStatus::Failed, _) => "review failed",
            _ if last.skipped => "review failed",
            _ => "reviewing",
        };
        Some(match reviews.len() {
            1 => word.to_owned(),
            count => format!("{word} \u{00B7} {count}"),
        })
    }

    /// "31 steps · 204k tokens", or none before it spent anything.
    pub fn spend(&self) -> Option<String> {
        (self.steps > 0 || self.tokens > 0).then(|| {
            format!(
                "{} \u{00B7} {} tokens",
                plural(self.steps, "step"),
                crate::thread::short_count(self.tokens)
            )
        })
    }
}

/// The task of `tasks` with id `task_id` and its number in the plan's rows.
pub fn find_task<'a>(tasks: &'a [Task], task_id: &str) -> Option<(usize, &'a Task)> {
    tasks
        .iter()
        .enumerate()
        .find(|(_, task)| task.task_id == task_id)
        .map(|(index, task)| (index + 1, task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_says_what_it_spent() {
        let task = Task {
            steps: 31,
            tokens: 204_000,
            ..Task::default()
        };
        assert_eq!(
            task.spend().as_deref(),
            Some("31 steps \u{00B7} 204k tokens")
        );
        assert_eq!(Task::default().spend(), None);
    }
}
