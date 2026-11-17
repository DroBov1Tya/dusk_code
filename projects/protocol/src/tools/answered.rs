//! An answer to the agent's question, read back from the message that carries it, so a
//! chat can show the question with its options instead of the raw text.

use super::{DECLINED, Question};
use crate::model::ToolCall;

/// One answered question as a chat draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub question: String,
    /// Every option offered and whether it was chosen; empty when the options are not
    /// known, such as for a call stored in an older form.
    pub options: Vec<(String, bool)>,
    /// The user's own words: beside the options, or instead of an answer.
    pub own: String,
    pub declined: bool,
}

impl Answered {
    /// What a card shows under the question: the chosen options joined by ", " (or
    /// "Declined"), then the user's own words. Empty when nothing was said.
    pub fn lines(&self) -> Vec<String> {
        let chosen: Vec<&str> = self
            .options
            .iter()
            .filter(|(_, chosen)| *chosen)
            .map(|(option, _)| option.as_str())
            .collect();
        let first = match (self.declined, chosen.is_empty()) {
            (true, _) => Some("Declined".to_owned()),
            (false, false) => Some(chosen.join(", ")),
            (false, true) => None,
        };
        let own = (!self.own.is_empty()).then(|| self.own.clone());
        first.into_iter().chain(own).collect()
    }
}

/// The questions a thread asked that nobody answered yet, as a client reads them from
/// the `ask_user` calls of the history or of the running turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked(Vec<Question>);

impl Asked {
    /// Notes the question of an `ask_user` call; any other call changes nothing.
    pub fn call(&mut self, call: &ToolCall) {
        if call.name != "ask_user" {
            return;
        }
        let question = serde_json::from_value(call.arguments.clone())
            .ok()
            .or_else(|| {
                // Arguments that are not a whole question still say what was asked.
                let question = call.arguments.get("question")?.as_str()?;
                Some(Question {
                    question: question.to_owned(),
                    options: Vec::new(),
                    multiple: false,
                })
            });
        self.0.extend(question);
    }

    /// The user's next message. Gives the answers when it answers what was asked, in
    /// the form `Question::answer` and `Question::declined` write, several questions
    /// one after another; `None` for any other message, whatever it looks like. Either
    /// way nothing is left asked: a later message is not an answer.
    pub fn answer(&mut self, message: &str) -> Option<Vec<Answered>> {
        let mut asked = std::mem::take(&mut self.0);
        let mut answered = Vec::new();
        let mut rest = message;
        loop {
            let at = asked
                .iter()
                .position(|question| rest.starts_with(&question.reply("")))?;
            let question = asked.remove(at);
            let body = &rest[question.reply("").len()..];
            // An answer ends where the next asked question begins.
            let next = asked
                .iter()
                .filter_map(|other| body.find(&format!("\n\n{}", other.reply(""))))
                .min();
            match next {
                Some(end) => {
                    answered.push(question.answered(&body[..end]));
                    rest = &body[end + 2..];
                }
                None => {
                    answered.push(question.answered(body));
                    return Some(answered);
                }
            }
        }
    }
}

impl Question {
    /// Reads back what `answer` or `declined` put after `ANSWER: `. Options joined by
    /// "; " can make choices and own text look the same. Ambiguous answers stay as text.
    fn answered(self, answer: &str) -> Answered {
        let mut chosen = vec![false; self.options.len()];
        let declined = answer == DECLINED
            || answer
                .strip_prefix(DECLINED)
                .is_some_and(|message| message.starts_with("\n\n"));
        let mut rest = if declined {
            &answer[DECLINED.len()..]
        } else {
            answer
        };
        while !declined && !rest.is_empty() && (self.multiple || !chosen.contains(&true)) {
            let mut matches = self.options.iter().enumerate().filter(|(index, option)| {
                !chosen[*index]
                    && !option.is_empty()
                    && rest
                        .strip_prefix(option.as_str())
                        .is_some_and(|after| after.is_empty() || after.starts_with("; "))
            });
            let Some((index, option)) = matches.next() else {
                break;
            };
            // Both interpretations could be valid selections or own text. Do not claim
            // a choice when the stored text cannot tell us which one was made.
            if matches.next().is_some() {
                chosen.fill(false);
                rest = answer;
                break;
            }
            chosen[index] = true;
            rest = &rest[option.len()..];
            rest = rest.strip_prefix("; ").unwrap_or(rest);
        }
        Answered {
            question: self.question,
            own: rest.trim().to_owned(),
            options: self.options.into_iter().zip(chosen).collect(),
            declined,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(text: &str, options: &[&str], multiple: bool) -> Question {
        Question {
            question: text.into(),
            options: options.iter().map(|&option| option.to_owned()).collect(),
            multiple,
        }
    }

    fn asked(questions: &[Question]) -> Asked {
        let mut asked = Asked::default();
        for question in questions {
            asked.call(&ToolCall {
                id: "1".into(),
                name: "ask_user".into(),
                arguments: serde_json::to_value(question).unwrap(),
            });
        }
        asked
    }

    fn chosen(answered: &Answered) -> Vec<&str> {
        let chosen = answered.options.iter().filter(|(_, chosen)| *chosen);
        chosen.map(|(option, _)| option.as_str()).collect()
    }

    #[test]
    fn chosen_options_and_own_words_read_back() {
        let way = question("Which way?", &["left", "right", "up"], true);
        let answers = asked(std::slice::from_ref(&way))
            .answer(&way.answer(&[2, 0], " or back "))
            .unwrap();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].question, "Which way?");
        assert_eq!(chosen(&answers[0]), ["left", "up"]);
        assert_eq!(answers[0].own, "or back");
        assert!(!answers[0].declined);
    }

    #[test]
    fn an_option_holding_the_separator_is_one_option() {
        let pick = question("Pick", &["a", "a; b", "b"], true);
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[1], ""))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "a; b");
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[0, 2], ""))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "a; b");
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[1, 0], "c; d"))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "a; b; a; c; d");
    }

    #[test]
    fn single_choice_with_separator_stays_text_when_ambiguous() {
        let pick = question("Pick", &["a", "a; b", "b"], false);
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[1], ""))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "a; b");
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[0], "b"))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "a; b");
    }

    #[test]
    fn one_choice_takes_one_option_and_the_rest_is_the_users() {
        let pick = question("Pick", &["a", "b"], false);
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.reply("a; b"))
            .unwrap();
        assert_eq!(chosen(&answers[0]), ["a"]);
        assert_eq!(answers[0].own, "b");
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[], "abc"))
            .unwrap();
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "abc");
    }

    #[test]
    fn declining_keeps_the_message_written_instead() {
        let pick = question("Pick", &["a", "b"], false);
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.declined("do it\n\nyour way"))
            .unwrap();
        assert!(answers[0].declined);
        assert!(chosen(&answers[0]).is_empty());
        assert_eq!(answers[0].own, "do it\n\nyour way");
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.declined(""))
            .unwrap();
        assert!(answers[0].declined && answers[0].own.is_empty());
        // The user's own words that only begin like a refusal are an answer.
        let answers = asked(std::slice::from_ref(&pick))
            .answer(&pick.answer(&[], "(declined) not"))
            .unwrap();
        assert!(!answers[0].declined);
    }

    #[test]
    fn several_questions_are_answered_by_one_message_in_any_order() {
        let first = question("One?", &["a", "b"], false);
        let second = question("Two?", &["c", "d"], false);
        let message = [second.declined("later"), first.answer(&[1], "")].join("\n\n");
        let answers = asked(&[first, second]).answer(&message).unwrap();
        assert_eq!(answers.len(), 2);
        assert!(answers[0].declined);
        assert_eq!(answers[0].own, "later");
        assert_eq!(answers[1].question, "One?");
        assert_eq!(chosen(&answers[1]), ["b"]);
    }

    #[test]
    fn text_that_only_looks_like_an_answer_is_not_one() {
        let pick = question("Pick", &["a", "b"], false);
        for message in [
            "",
            "QUESTION: Pick",
            "QUESTION: Pick\nANSWER:",
            "QUESTION: Other\nANSWER: a",
            " QUESTION: Pick\nANSWER: a",
            "QUESTION: Pick\nANSWER\u{00A0}: a",
        ] {
            assert_eq!(
                asked(std::slice::from_ref(&pick)).answer(message),
                None,
                "{message:?}"
            );
        }
        // Nothing was asked, and a question stays asked for one message only.
        assert_eq!(Asked::default().answer(&pick.answer(&[0], "")), None);
        let mut asked = asked(std::slice::from_ref(&pick));
        assert_eq!(asked.answer("something else"), None);
        assert_eq!(asked.answer(&pick.answer(&[0], "")), None);
    }

    #[test]
    fn a_call_without_options_still_shows_its_question() {
        let mut asked = Asked::default();
        for (name, arguments) in [
            (
                "ask_user",
                serde_json::json!({ "question": "Pick", "options": "no" }),
            ),
            ("ask_user", serde_json::json!({ "options": ["a", "b"] })),
            ("ask_user", serde_json::json!("QUESTION")),
            (
                "bash",
                serde_json::json!({ "question": "Other", "options": ["a", "b"] }),
            ),
        ] {
            asked.call(&ToolCall {
                id: "1".into(),
                name: name.into(),
                arguments,
            });
        }
        let answers = asked.answer("QUESTION: Pick\nANSWER: a; я").unwrap();
        assert_eq!(answers.len(), 1);
        assert!(answers[0].options.is_empty());
        assert_eq!(answers[0].own, "a; я");
    }
}
