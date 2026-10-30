//! A question the agent asked, shown above the message box until it is answered. The
//! options come first and the user's own answer last. A single choice is sent with
//! Enter on it; with several, Space marks them and Enter sends. Esc or the button
//! declines, and a message sent from the box declines it as well.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dusk_core::tools::Question;

use super::{Action, App, Input};

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
            KeyCode::Down => poll.cursor = (poll.cursor + 1).min(poll.own_row()),
            KeyCode::Enter => return self.press_poll(PollTarget::Send),
            KeyCode::Char(' ') if !on_own => {
                if poll.question.multiple {
                    poll.checked[poll.cursor] ^= true;
                }
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
        self.poll = None;
        self.send_text(text)
    }

    /// Text that answers the question goes as a message, after the turn if one runs.
    fn send_text(&mut self, text: String) -> Action {
        if self.is_busy() {
            self.queued.push(text);
            return Action::None;
        }
        self.show_sent(&text);
        Action::Send(text)
    }
}
