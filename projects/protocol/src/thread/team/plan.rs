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
        let verdict = last.result.lines().rev().find_map(verdict_word);
        // An approval that names a high problem sends the task back all the same.
        let high = last.result.lines().any(high_problem);
        let verdict = match verdict.as_deref() {
            Some("approve" | "approved") if high => Some("fix".to_owned()),
            _ => verdict,
        };
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

/// Words and phrases that make a remark after an approval a condition on it (`approve -
/// but fix the docs first`, `approve once CI is green`): such an approval is no clear
/// verdict. Words that only tell when or how a check ran (`after the rebuild`, `when run
/// locally`) are no condition. The core reads it the same.
const CONDITIONS: [&str; 12] = [
    "but",
    "once",
    "provided",
    "if",
    "as long as",
    "pending",
    "except",
    "after you fix",
    "after they fix",
    "need to",
    "needs to",
    "unless",
];

/// Whether the remark after a verdict's word puts a condition on it.
pub fn conditional_remark(remark: &str) -> bool {
    let words: Vec<String> = remark
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let said = format!(" {} ", words.join(" "));
    CONDITIONS
        .iter()
        .any(|condition| said.contains(&format!(" {condition} ")))
}

/// The value of a stored line labelled `name`, written as a member wrote it:
/// `Verdict: approve`, `- **Verdict:** Approve.`. None for another line.
fn label_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let line = line.trim();
    let line = ["- ", "* ", "+ ", "\u{2022} "]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .unwrap_or(line)
        .trim_start_matches(['*', '_', '#', ' ']);
    let head = line.get(..name.len())?;
    if !head.eq_ignore_ascii_case(name) {
        return None;
    }
    let value = line[name.len()..]
        .trim_start_matches(['*', '_'])
        .trim_start();
    Some(
        value
            .strip_prefix(':')?
            .trim()
            .trim_matches(['*', '_', '`'])
            .trim(),
    )
}

/// Whether a stored line is a `Problem:` of high severity, its severity in markdown or not.
fn high_problem(line: &str) -> bool {
    label_value(line, "problem").is_some_and(|value| {
        let severity = value.split('|').next().unwrap_or_default();
        severity
            .trim()
            .trim_matches(['*', '_', '`'])
            .eq_ignore_ascii_case("high")
    })
}

/// The word of a stored review's `Verdict:` line, written as the reviewer wrote it:
/// `Verdict: approve`, `- **Verdict:** Approve.`, `verdict: fix (two problems)`,
/// `Verdict: approve \u{2705}`. None for another line, a value that is not one word, or
/// an approval with a condition (`approve - but fix the docs first`).
fn verdict_word(line: &str) -> Option<String> {
    let value = label_value(line, "verdict")?;
    let cut = value
        .find(|c: char| c.is_whitespace() || matches!(c, '(' | ':'))
        .unwrap_or(value.len());
    let (word, rest) = value.split_at(cut);
    let rest = rest.trim();
    let adorned = rest.chars().all(adornment);
    let remark =
        adorned || rest.starts_with(['(', '-', '\u{2013}', '\u{2014}', ':']) || word.ends_with('.');
    let word = word
        .trim_matches(['*', '_', '`'])
        .trim_end_matches(adornment)
        .to_ascii_lowercase();
    let unclear = matches!(word.as_str(), "approve" | "approved") && conditional_remark(rest);
    (remark && !unclear).then_some(word)
}

/// An emoji, a mark, a `!` or a `.` after a word: no part of what it says.
fn adornment(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '!' | '.' | '*' | '_' | '`')
        || !c.is_ascii() && !c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_chip_reads_a_verdict_in_markdown() {
        let task = Task {
            task_id: "t1".into(),
            ..Task::default()
        };
        let review = |result: &str| Task {
            task_id: "r1".into(),
            duty: TaskDuty::Review,
            review_of: Some("t1".into()),
            status: TaskStatus::Done,
            result: result.into(),
            ..Task::default()
        };
        for (result, word) in [
            ("Verdict: approve\nChecked: x", "reviewed"),
            ("- **Verdict:** Approve.\n- **Checked:** x", "reviewed"),
            ("**Verdict**: fix (two problems)\nChecked: x", "review: fix"),
            ("Verdict: approve or fix\nChecked: x", "review: no verdict"),
            ("Checked: x", "review: no verdict"),
            ("Verdict: approve \u{2705}\nChecked: x", "reviewed"),
            ("Verdict: **Approve!**\nChecked: x", "reviewed"),
            (
                "Verdict: approve - but fix the docs first\nChecked: x",
                "review: no verdict",
            ),
            (
                "Problem: **high** | a.rs | wrong | fix\nVerdict: approve\nChecked: x",
                "review: fix",
            ),
        ] {
            let tasks = [task.clone(), review(result)];
            assert_eq!(task.review_word(&tasks).as_deref(), Some(word), "{result}");
        }
    }

    #[test]
    fn only_a_condition_holds_an_approval() {
        for (remark, held) in [
            ("(all checks pass after the rebuild)", false),
            ("- looks good, though naming is odd", false),
            ("(verified when run locally)", false),
            ("(first pass clean)", false),
            ("- but fix docs", true),
            ("once CI is green", true),
            ("(need to bump version)", true),
            ("(pending CI)", true),
            ("- as long as the docs land", true),
            ("- after you fix the test", true),
            ("(unless the build fails)", true),
        ] {
            assert_eq!(conditional_remark(remark), held, "{remark}");
        }
    }

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
