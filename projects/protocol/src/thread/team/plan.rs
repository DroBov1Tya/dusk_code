//! A plan's tasks in the words, glyphs and tones every client shows.

use crate::thread::{Task, TaskStatus, Tone, plural};

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
