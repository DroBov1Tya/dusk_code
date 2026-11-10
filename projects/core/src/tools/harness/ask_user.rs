//! `ask_user`: a question to the user with options to choose from, shown by the client
//! above the message box. The call returns at once; the user's choice comes back as
//! their next message, in the form `answer` and `declined` write, so the model reads it
//! the same whichever client asked.

use anyhow::Context;
use serde_json::Value;

use crate::tools::{
    schema,
    schema::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{BoxFuture, runtime::ThreadEvent};

pub use dusk_protocol::tools::Question;
use dusk_protocol::tools::{MAX_OPTIONS, MIN_OPTIONS};

pub struct AskUser;

impl Tool for AskUser {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Ask the user a question with options to choose from, when something missing \
             would change the result and you cannot find it out yourself; not at every step, \
             and not for what has a sensible default. One question at a time, at most three \
             in a row; afterwards say what you assumed. List the option you recommend first. \
             Set `multiple` when several options may be chosen. The \
             user can always write their own answer or decline. After calling it, end \
             your turn without writing more: the answer comes as the user's next message, \
             as QUESTION: ... ANSWER: .... {MIN_OPTIONS} to {MAX_OPTIONS} short options."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Question>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
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
