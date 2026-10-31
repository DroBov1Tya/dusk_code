//! `ask_user`: a question to the user with options to choose from, shown by the client
//! above the message box. The call returns at once; the user's choice comes back as
//! their next message, in the form `answer` and `declined` write, so the model reads it
//! the same whichever client asked.

use anyhow::{Context, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{
    builtin::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{BoxFuture, runtime::ThreadEvent};

const MAX_QUESTION_CHARS: usize = 300;
const MAX_OPTION_CHARS: usize = 120;
const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 8;

/// A question as the client shows it. The client always adds a field for the user's
/// own answer below the options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub question: String,
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

    fn reply(&self, answer: &str) -> String {
        format!("QUESTION: {}\nANSWER: {answer}", self.question)
    }

    fn check(&self) -> anyhow::Result<()> {
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

pub struct AskUser;

impl Tool for AskUser {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        format!(
            "Ask the user a question with options to choose from, when something missing \
             would change the result and you cannot find it out yourself; not at every step, \
             and not for what has a sensible default. List the option you recommend first. \
             Set `multiple` when several options may be chosen. The \
             user can always write their own answer or decline. After calling it, end \
             your turn without writing more: the answer comes as the user's next message, \
             as QUESTION: ... ANSWER: .... {MIN_OPTIONS} to {MAX_OPTIONS} short options."
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string" },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": MIN_OPTIONS,
                    "maxItems": MAX_OPTIONS
                },
                "multiple": {
                    "type": "boolean",
                    "description": "Several options may be chosen. False by default."
                }
            },
            "required": ["question", "options"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let question: Question = parse_arguments(arguments)?;
            question.check()?;
            let progress = context
                .progress
                .context("there is no user to ask in this thread")?;
            progress.emit(ThreadEvent::Question(question));
            Ok(
                "The question is on the user's screen. End your turn now; their answer \
                comes as their next message."
                    .into(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(options: &[&str]) -> Question {
        Question {
            question: "Which way?".into(),
            options: options.iter().map(|&option| option.to_owned()).collect(),
            multiple: true,
        }
    }

    #[test]
    fn answers_read_as_question_and_answer() {
        let asked = question(&["left", "right"]);
        assert!(asked.check().is_ok());
        assert_eq!(
            asked.answer(&[1, 0], " up "),
            "QUESTION: Which way?\nANSWER: right; left; up"
        );
        assert_eq!(
            asked.declined("go back"),
            "QUESTION: Which way?\nANSWER: (declined)\n\ngo back"
        );
        assert!(question(&["only"]).check().is_err());
        assert!(question(&["same", " same"]).check().is_err());
    }
}
