//! A plan's tasks and counts in the words, glyphs and tones every client shows.

use crate::thread::{PlanCounts, Task, TaskStatus, Tone};

impl TaskStatus {
    /// Given out or running: a member works on it.
    pub fn at_work(self) -> bool {
        matches!(self, Self::Claimed | Self::Running)
    }

    /// Done or failed: nothing more happens to it unless the lead acts.
    pub fn over(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }

    /// The word a row says for it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Claimed => "starting",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Review => "in review",
            Self::Done => "done",
            Self::Failed => "failed",
        }
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

impl PlanCounts {
    pub fn total(self) -> u32 {
        [
            self.open,
            self.claimed,
            self.running,
            self.blocked,
            self.review,
            self.done,
            self.failed,
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }

    /// Tasks given out or running.
    pub fn at_work(self) -> u32 {
        self.claimed.saturating_add(self.running)
    }

    /// "3 of 7 done · 1 running · 1 blocked": the parts that are not zero after the done.
    pub fn summary(self) -> String {
        let mut text = format!("{} of {} done", self.done, self.total());
        for (count, status) in [
            (self.running, TaskStatus::Running),
            (self.claimed, TaskStatus::Claimed),
            (self.review, TaskStatus::Review),
            (self.blocked, TaskStatus::Blocked),
            (self.failed, TaskStatus::Failed),
        ] {
            if count > 0 {
                text.push_str(&format!(" \u{00B7} {count} {}", status.word()));
            }
        }
        text
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
                count(u64::from(self.steps), "step"),
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

/// "1 step", "31 steps".
pub(super) fn count(value: u64, noun: &str) -> String {
    match value {
        1 => format!("1 {noun}"),
        _ => format!("{value} {noun}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_sums_up_its_counts_in_the_words_of_its_rows() {
        let counts = PlanCounts {
            open: 2,
            claimed: 1,
            running: 1,
            blocked: 1,
            review: 1,
            done: 3,
            failed: 0,
        };
        assert_eq!(counts.total(), 9);
        assert_eq!(counts.at_work(), 2);
        assert_eq!(
            counts.summary(),
            "3 of 9 done \u{00B7} 1 running \u{00B7} 1 starting \u{00B7} 1 in review \u{00B7} 1 blocked"
        );
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
