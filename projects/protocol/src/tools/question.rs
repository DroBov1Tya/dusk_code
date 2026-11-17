//! The question an agent asks the user.

use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How long a question and its options may be, and how many options it has.
pub const MAX_QUESTION_CHARS: usize = 300;

pub const MAX_OPTION_CHARS: usize = 120;

pub const MIN_OPTIONS: usize = 2;

pub const MAX_OPTIONS: usize = 8;

/// What stands for the answer when the user would not give one.
pub const DECLINED: &str = "(declined)";

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
        let reply = self.reply(DECLINED);
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

/// One question with what the user answered, read back from the message that carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub question: String,
    pub answer: String,
}

impl Reply {
    /// The replies of a message built by `Question::answer` or `declined`, one per question
    /// asked in a row; none when the text is anything else, so it shows as a plain message.
    pub fn parse_all(text: &str) -> Option<Vec<Reply>> {
        text.strip_prefix("QUESTION: ")?
            .split("\n\nQUESTION: ")
            .map(|block| {
                let (question, answer) = block.split_once("\nANSWER: ")?;
                (!question.trim().is_empty()).then(|| Reply {
                    question: question.trim().to_owned(),
                    answer: answer.trim().to_owned(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(text: &str) -> Question {
        Question {
            question: text.into(),
            options: vec!["a".into(), "b".into()],
            multiple: false,
        }
    }

    #[test]
    fn replies_are_read_back_from_the_answers_that_wrote_them() {
        let text = [
            asked("One?").answer(&[0, 1], "own"),
            asked("Two?").declined("later"),
        ]
        .join("\n\n");
        assert_eq!(
            Reply::parse_all(&text),
            Some(vec![
                Reply {
                    question: "One?".into(),
                    answer: "a; b; own".into()
                },
                Reply {
                    question: "Two?".into(),
                    answer: "(declined)\n\nlater".into()
                },
            ])
        );
    }

    #[test]
    fn other_text_is_not_a_reply() {
        assert_eq!(Reply::parse_all("hello"), None);
        assert_eq!(Reply::parse_all("QUESTION: no answer here"), None);
        assert_eq!(Reply::parse_all("QUESTION: \nANSWER: x"), None);
    }
}
