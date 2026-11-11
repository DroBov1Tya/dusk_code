//! The question an agent asks the user.

use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How long a question and its options may be, and how many options it has.
pub const MAX_QUESTION_CHARS: usize = 300;

pub const MAX_OPTION_CHARS: usize = 120;

pub const MIN_OPTIONS: usize = 2;

pub const MAX_OPTIONS: usize = 8;

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

    /// The message saying the user would not answer, ahead of what they wrote instead.
    pub fn declined(&self, message: &str) -> String {
        let reply = self.reply("(declined)");
        match message.trim() {
            "" => reply,
            message => format!("{reply}\n\n{message}"),
        }
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
