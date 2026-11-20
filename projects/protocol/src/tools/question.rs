//! The question an agent asks the user.

use std::collections::VecDeque;

use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::thread::{CHOICE_CHARS, MAX_CHOICES, MIN_CHOICES};

/// How long a question and its options may be, and how many options it has.
pub const MAX_QUESTION_CHARS: usize = 300;

pub const MAX_OPTION_CHARS: usize = 120;

pub const MIN_OPTIONS: usize = 2;

pub const MAX_OPTIONS: usize = 8;

/// What stands for the answer when the user would not give one.
pub const DECLINED: &str = "(declined)";

/// What stands for the answer when the user skipped this one question and left it to the
/// agent; the questions after it are still answered.
pub const SKIPPED: &str = "(skipped \u{2014} use your own judgement)";

/// A question as the client shows it. The client always adds a field for the user's
/// own answer below the options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub question: String,
    #[schemars(length(min = MIN_OPTIONS, max = MAX_OPTIONS))]
    pub options: Vec<String>,
    /// Several options may be chosen; otherwise exactly one, or the user's own answer.
    #[serde(default)]
    pub multiple: bool,
}

impl Question {
    /// The message that answers it: the chosen options, by index, then the user's own
    /// words, if any.
    pub fn answer(&self, chosen: &[usize], own: &str) -> String {
        let mut parts: Vec<&str> = chosen
            .iter()
            .filter_map(|&index| self.options.get(index))
            .map(String::as_str)
            .collect();
        let own = own.trim();
        if !own.is_empty() {
            parts.push(own);
        }
        self.reply(&parts.join("; "))
    }

    /// Whether the question is short enough to be drawn as reply buttons instead of a
    /// card: one answer, 2 to 4 options, each a non-empty label of `CHOICE_CHARS` at
    /// most. Options are the model's text, so they are measured in characters, trimmed.
    pub fn light(&self) -> bool {
        !self.multiple
            && (MIN_CHOICES..=MAX_CHOICES).contains(&self.options.len())
            && self.options.iter().all(|option| {
                let chars = option.trim().chars().count();
                (1..=CHOICE_CHARS).contains(&chars)
            })
    }

    /// The message saying the user would not answer, ahead of what they wrote instead.
    pub fn declined(&self, message: &str) -> String {
        let reply = self.reply(DECLINED);
        match message.trim() {
            "" => reply,
            message => format!("{reply}\n\n{message}"),
        }
    }

    /// The message saying the user skipped this question: the agent decides itself.
    pub fn skipped(&self) -> String {
        self.reply(SKIPPED)
    }

    pub fn reply(&self, answer: &str) -> String {
        format!("QUESTION: {}\nANSWER: {answer}", self.question)
    }

    pub fn check(&self) -> anyhow::Result<()> {
        let question = self.question.trim();
        ensure!(!question.is_empty(), "the question is empty");
        ensure!(
            question.chars().count() <= MAX_QUESTION_CHARS,
            "the question is over {MAX_QUESTION_CHARS} characters"
        );
        ensure!(
            (MIN_OPTIONS..=MAX_OPTIONS).contains(&self.options.len()),
            "give {MIN_OPTIONS} to {MAX_OPTIONS} options"
        );
        for (index, option) in self.options.iter().enumerate() {
            let option = option.trim();
            ensure!(!option.is_empty(), "option {} is empty", index + 1);
            ensure!(
                option.chars().count() <= MAX_OPTION_CHARS,
                "option {} is over {MAX_OPTION_CHARS} characters",
                index + 1
            );
            ensure!(
                !self.options[..index]
                    .iter()
                    .any(|other| other.trim() == option),
                "option {} repeats an earlier one",
                index + 1
            );
        }
        Ok(())
    }
}

/// The questions of one round: the one shown, those asked while it was, and the answers
/// given so far. A second question waits behind the first instead of taking its place,
/// and every answer goes to the agent together, after the last one, as the core reads
/// them: one reply a question, apart by a blank line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionQueue {
    shown: Option<Question>,
    waiting: VecDeque<Question>,
    answers: Vec<String>,
}

impl QuestionQueue {
    /// Adds a question; true when it is the one shown now.
    pub fn ask(&mut self, question: Question) -> bool {
        if self.shown.is_some() {
            self.waiting.push_back(question);
            false
        } else {
            self.shown = Some(question);
            true
        }
    }

    pub fn shown(&self) -> Option<&Question> {
        self.shown.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_none()
    }

    /// The shown question's number and how many this round has: `(2, 3)` reads "2 of 3".
    pub fn place(&self) -> (usize, usize) {
        let number = self.answers.len() + 1;
        (number, number + self.waiting.len())
    }

    /// Keeps `reply` as the shown question's answer and shows the next one. The message
    /// for the agent once every question has its answer; none when one still waits or
    /// nothing was shown.
    pub fn reply(&mut self, reply: String) -> Option<String> {
        self.shown.as_ref()?;
        self.answers.push(reply);
        self.shown = self.waiting.pop_front();
        self.is_empty().then(|| self.take())
    }

    /// Answers the shown question with the chosen options and the user's own words.
    pub fn answer(&mut self, chosen: &[usize], own: &str) -> Option<String> {
        let reply = self.shown.as_ref()?.answer(chosen, own);
        self.reply(reply)
    }

    /// Skips the shown question and only that one: the agent decides it itself.
    pub fn skip(&mut self) -> Option<String> {
        let reply = self.shown.as_ref()?.skipped();
        self.reply(reply)
    }

    /// The user wrote `message` instead of answering: the shown question is declined, the
    /// ones waiting go unasked, and every answer goes now. None when nothing was shown.
    pub fn decline(&mut self, message: &str) -> Option<String> {
        let reply = self.shown.take()?.declined(message);
        self.waiting.clear();
        self.answers.push(reply);
        Some(self.take())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn take(&mut self) -> String {
        std::mem::take(&mut self.answers).join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(options: &[&str], multiple: bool) -> Question {
        Question {
            question: "Which?".into(),
            options: options.iter().map(|o| o.to_string()).collect(),
            multiple,
        }
    }

    #[test]
    fn a_question_is_light_only_within_every_cap() {
        let edge = "\u{e9}".repeat(CHOICE_CHARS);
        let over = "\u{e9}".repeat(CHOICE_CHARS + 1);
        assert!(question(&["Yes", "No"], false).light());
        assert!(question(&[&edge, "b", "c", "d"], false).light());
        assert!(!question(&["Yes"], false).light());
        assert!(!question(&["a", "b", "c", "d", "e"], false).light());
        assert!(!question(&["Yes", "No"], true).light());
        assert!(!question(&[&over, "No"], false).light());
        assert!(!question(&["Yes", "  "], false).light());
        assert!(question(&[&format!(" {edge} "), "No"], false).light());
    }

    #[test]
    fn a_skipped_question_leaves_the_decision_to_the_agent() {
        let asked = question(&["Yes", "No"], false);
        assert_eq!(
            asked.skipped(),
            "QUESTION: Which?\nANSWER: (skipped \u{2014} use your own judgement)"
        );
        assert_ne!(asked.skipped(), asked.declined(""));
    }

    #[test]
    fn a_queue_sends_every_answer_after_the_last_question() {
        let (first, second) = (question(&["a", "b"], false), question(&["c", "d"], true));
        let mut queue = QuestionQueue::default();
        assert!(queue.ask(first.clone()));
        assert!(!queue.ask(second.clone()), "a second question waits");
        assert_eq!(queue.shown(), Some(&first));
        assert_eq!(queue.place(), (1, 2));
        assert_eq!(queue.skip(), None);
        assert_eq!(queue.shown(), Some(&second));
        assert_eq!(queue.place(), (2, 2));
        let sent = queue.answer(&[1], "").unwrap();
        assert_eq!(
            sent,
            [first.skipped(), second.answer(&[1], "")].join("\n\n")
        );
        assert!(queue.is_empty());
        assert_eq!(queue.skip(), None, "nothing is left to skip");
        assert_eq!(queue.reply("late".into()), None);
        assert_eq!(
            queue.place(),
            (1, 1),
            "a reply with nothing shown is dropped"
        );
    }

    #[test]
    fn declining_drops_the_waiting_questions_and_sends_at_once() {
        let (first, second, third) = (
            question(&["a", "b"], false),
            question(&["c", "d"], false),
            question(&["e", "f"], false),
        );
        let mut queue = QuestionQueue::default();
        for asked in [&first, &second, &third] {
            queue.ask(asked.clone());
        }
        assert_eq!(queue.answer(&[0], ""), None);
        let sent = queue.decline("do it later").unwrap();
        assert_eq!(
            sent,
            [first.answer(&[0], ""), second.declined("do it later")].join("\n\n")
        );
        assert!(queue.is_empty());
        assert_eq!(queue.place(), (1, 1));
        assert_eq!(queue.decline("again"), None);
    }
}
