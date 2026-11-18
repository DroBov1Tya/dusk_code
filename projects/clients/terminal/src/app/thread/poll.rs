//! A question the agent asked, shown above the message box until it is answered. The
//! options come first and the user's own answer last. A single choice is sent with
//! Enter on it; with several, Space marks them and Enter sends. Esc or the button
//! declines, and a message sent from the box declines it as well. A question short enough
//! to be reply buttons (`Question::light`) is drawn as the reply row instead of the card,
//! and answered with a number key as well.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dusk_protocol::{thread::Block, tools::Question};

use super::{Action, App, Entry, Input};

pub struct Poll {
    pub question: Question,
    /// The row under the cursor: an option, or the own answer after them.
    pub cursor: usize,
    pub checked: Vec<bool>,
    pub own: Input,
    /// Keys go to the question rather than the message box.
    pub active: bool,
    pub hits: Vec<PollHit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PollTarget {
    Row(usize),
    Send,
    Decline,
}

/// Something clickable in the question on the last drawn frame.
pub struct PollHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub target: PollTarget,
}

impl Poll {
    pub fn new(question: Question) -> Self {
        Self {
            checked: vec![false; question.options.len()],
            question,
            cursor: 0,
            own: Input::default(),
            active: true,
            hits: Vec::new(),
        }
    }

    /// The row of the own answer.
    pub fn own_row(&self) -> usize {
        self.question.options.len()
    }

    /// The last row the cursor reaches: a light question has no row for the own answer, since
    /// it is not drawn; a message written in the box answers it by declining.
    fn last_row(&self) -> usize {
        match self.question.light() {
            true => self.own_row() - 1,
            false => self.own_row(),
        }
    }

    /// The answer as it stands, or `None` while nothing is chosen or written.
    fn answer(&self) -> Option<String> {
        let chosen: Vec<usize> = if self.question.multiple {
            (0..self.checked.len())
                .filter(|&at| self.checked[at])
                .collect()
        } else if self.cursor < self.own_row() {
            vec![self.cursor]
        } else {
            Vec::new()
        };
        if chosen.is_empty() && self.own.text.trim().is_empty() {
            return None;
        }
        Some(self.question.answer(&chosen, &self.own.text))
    }
}

impl App {
    pub fn poll_key(&mut self, key: KeyEvent) -> Action {
        let Some(poll) = &mut self.poll else {
            return Action::None;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let on_own = poll.cursor == poll.own_row();
        match key.code {
            KeyCode::Esc => return self.press_poll(PollTarget::Decline),
            KeyCode::Tab => poll.active = false,
            KeyCode::Up => poll.cursor = poll.cursor.saturating_sub(1),
            KeyCode::Down => poll.cursor = (poll.cursor + 1).min(poll.last_row()),
            KeyCode::Enter => return self.press_poll(PollTarget::Send),
            KeyCode::Char(' ') if !on_own && poll.question.multiple => {
                poll.checked[poll.cursor] ^= true;
            }
            KeyCode::Char(' ') if !on_own => {}
            // The number on the reply row, for terminals that do not send Alt+number.
            KeyCode::Char(digit @ '1'..='4') if poll.question.light() && !ctrl => {
                return self.press_reply(digit as usize - '1' as usize);
            }
            KeyCode::Char(c) if on_own && !ctrl => poll.own.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace if on_own => poll.own.backspace(),
            KeyCode::Delete if on_own => poll.own.delete(),
            KeyCode::Left if on_own => poll.own.left(),
            KeyCode::Right if on_own => poll.own.right(),
            KeyCode::Home if on_own => poll.own.home(),
            KeyCode::End if on_own => poll.own.end(),
            _ => {}
        }
        Action::None
    }

    pub fn press_poll(&mut self, target: PollTarget) -> Action {
        let Some(poll) = &mut self.poll else {
            return Action::None;
        };
        poll.active = true;
        let text = match target {
            PollTarget::Row(row) => {
                if poll.question.multiple && row < poll.own_row() && poll.cursor == row {
                    poll.checked[row] ^= true;
                }
                poll.cursor = row;
                return Action::None;
            }
            PollTarget::Decline => poll.question.declined(""),
            PollTarget::Send => match poll.answer() {
                Some(text) => text,
                None => {
                    self.flash = Some("choose an option or write your own".into());
                    return Action::None;
                }
            },
        };
        self.answer_with(text)
    }

    fn answer_with(&mut self, text: String) -> Action {
        match self.answered(text) {
            Some(text) => self.send_text(text),
            None => Action::None,
        }
    }

    /// The labels of the reply row, in the order of their numbers: those of a light
    /// question waiting for an answer, or of a `choices` block that is the last thing in
    /// the thread once the turn has ended. Empty when there is no row.
    pub fn reply_labels(&self) -> Vec<&str> {
        if !self.approvals.is_empty() {
            return Vec::new();
        }
        if let Some(poll) = &self.poll {
            return match poll.question.light() {
                true => poll.question.options.iter().map(|o| o.trim()).collect(),
                false => Vec::new(),
            };
        }
        if self.is_busy() {
            return Vec::new();
        }
        // An activity line is the app's own summary of steps, not something said after.
        let last = self
            .transcript
            .entries
            .iter()
            .rfind(|entry| !matches!(entry, Entry::Activity { .. }));
        match last {
            Some(Entry::Block(Block::Choices(choices))) => choices
                .labels()
                .iter()
                .map(|label| label.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Alt+number or the number: answers with the label of that row, as the card or the
    /// message box would have sent it. `None` when no such label is on the row.
    pub fn reply_key(&mut self, digit: char) -> Option<Action> {
        let index = (digit as usize).checked_sub('1' as usize)?;
        (index < self.reply_labels().len()).then(|| self.press_reply(index))
    }

    fn press_reply(&mut self, index: usize) -> Action {
        if let Some(poll) = &mut self.poll {
            if !poll.question.light() || index >= poll.question.options.len() {
                return Action::None;
            }
            let text = poll.question.answer(&[index], "");
            return self.answer_with(text);
        }
        let Some(label) = self
            .reply_labels()
            .get(index)
            .map(|label| (*label).to_owned())
        else {
            return Action::None;
        };
        self.show_sent(&label);
        Action::Send(label)
    }

    /// Keeps the answer to the question shown and shows the next one. The message for
    /// the agent once every question asked has its answer.
    pub fn answered(&mut self, text: String) -> Option<String> {
        self.poll_answers.push(text);
        self.poll = self.polls_waiting.pop_front().map(Poll::new);
        self.poll
            .is_none()
            .then(|| std::mem::take(&mut self.poll_answers).join("\n\n"))
    }

    /// The answer goes at once, even while the thread works: the core lets it join the
    /// running turn, which may be waiting for it.
    fn send_text(&mut self, text: String) -> Action {
        self.show_sent(&text);
        Action::Send(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Activity, TabMove, test_app};
    use crossterm::event::Event;
    use dusk_protocol::model::ToolCall;
    use serde_json::json;

    fn press(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Action {
        app.handle(Event::Key(KeyEvent::new(key, modifiers)))
    }

    fn question(options: &[&str], multiple: bool) -> Question {
        Question {
            question: "Which?".into(),
            options: options.iter().map(|option| option.to_string()).collect(),
            multiple,
        }
    }

    /// A thread whose last entry is a `choices` block the agent showed.
    fn with_choices(labels: &[&str]) -> App {
        let mut app = test_app();
        app.transcript.add_user("go");
        let call = ToolCall {
            id: "a".into(),
            name: "show".into(),
            arguments: json!({ "choices": { "labels": labels } }),
        };
        let block = Block::asked(&call).expect("a block");
        app.transcript.entries_mut().push(Entry::Block(block));
        app
    }

    #[test]
    fn alt_number_answers_a_light_question_as_its_card_would() {
        let asked = question(&["Yes", "No", "Later"], false);
        let mut app = test_app();
        app.poll = Some(Poll::new(asked.clone()));
        assert_eq!(app.reply_labels(), ["Yes", "No", "Later"]);
        let action = press(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
        assert!(matches!(action, Action::Send(text) if text == asked.answer(&[1], "")));
        assert!(app.poll.is_none());
    }

    #[test]
    fn a_plain_number_answers_a_light_question_too() {
        let asked = question(&["Yes", "No"], false);
        let mut app = test_app();
        app.poll = Some(Poll::new(asked.clone()));
        let action = press(&mut app, KeyCode::Char('1'), KeyModifiers::NONE);
        assert!(matches!(action, Action::Send(text) if text == asked.answer(&[0], "")));
        // A number past the last label is nothing, and Enter still sends the row under the cursor.
        app.poll = Some(Poll::new(asked.clone()));
        assert!(matches!(
            press(&mut app, KeyCode::Char('3'), KeyModifiers::NONE),
            Action::None
        ));
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        let action = press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(action, Action::Send(text) if text == asked.answer(&[1], "")));
    }

    #[test]
    fn a_question_that_is_not_light_keeps_its_card_and_gets_no_row() {
        for asked in [
            question(&["a", "b"], true),
            question(&["a", "b", "c", "d", "e"], false),
            question(&["a", &"b".repeat(25)], false),
        ] {
            let mut app = test_app();
            app.poll = Some(Poll::new(asked));
            assert!(app.reply_labels().is_empty());
            let action = press(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
            assert!(matches!(action, Action::Tab(TabMove::Go(0))));
            assert!(app.poll.is_some());
            // The digit is the own answer's text on the card, as before.
            press(&mut app, KeyCode::Char('1'), KeyModifiers::NONE);
            assert!(app.poll.is_some());
        }
    }

    #[test]
    fn choices_are_a_row_only_while_last_and_the_turn_is_over() {
        let mut app = with_choices(&["Yes", "No"]);
        assert_eq!(app.reply_labels(), ["Yes", "No"]);
        app.activity = Activity::Thinking;
        assert!(app.reply_labels().is_empty(), "the turn has not ended");
        app.activity = Activity::Idle;
        app.transcript.add_answer("Anything else?");
        assert!(app.reply_labels().is_empty(), "something came after");
    }

    #[test]
    fn alt_number_sends_the_choice_as_the_next_message() {
        let mut app = with_choices(&["Yes", "No"]);
        let action = press(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
        assert!(matches!(action, Action::Send(text) if text == "No"));
        assert!(matches!(
            app.transcript.entries.last(),
            Some(Entry::User { text, .. }) if text == "No"
        ));
        assert!(
            app.reply_labels().is_empty(),
            "the message came after the row"
        );
        // With no row, Alt+number is the tab switch again; so is a number past the labels.
        let action = press(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
        assert!(matches!(action, Action::Tab(TabMove::Go(0))));
        let mut app = with_choices(&["Yes", "No"]);
        let action = press(&mut app, KeyCode::Char('3'), KeyModifiers::ALT);
        assert!(matches!(action, Action::Tab(TabMove::Go(2))));
    }

    #[test]
    fn a_pending_approval_hides_the_row() {
        let mut app = with_choices(&["Yes", "No"]);
        app.approvals.push(crate::app::approval_batch(1, "ls"));
        assert!(app.reply_labels().is_empty());
    }
}
