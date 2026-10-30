//! Messages written while the agent works wait above the box instead of joining the
//! running turn. When the turn ends they go as one message; each can also go at once,
//! interrupting the agent, be taken back into the box, or be dropped.

use super::{Action, App, Entry};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QueueButton {
    /// Back into the box to edit.
    Edit,
    SendNow,
    Remove,
}

/// A queued message's button on the last drawn frame.
pub struct QueueHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
    pub button: QueueButton,
}

impl App {
    /// The queued messages as one, shown as sent; `None` when nothing waits.
    pub fn release_queue(&mut self) -> Option<String> {
        if self.queued.is_empty() {
            return None;
        }
        let text = self.queued.drain(..).collect::<Vec<_>>().join("\n\n");
        self.show_sent(&text);
        Some(text)
    }

    /// Stopping the agent hands what waits back to the box rather than sending it.
    pub fn unqueue_into_box(&mut self) {
        if !self.queued.is_empty() && self.input.text.is_empty() {
            let text = self.queued.drain(..).collect::<Vec<_>>().join("\n\n");
            self.input.set(text);
        }
    }

    pub fn press_queued(&mut self, index: usize, button: QueueButton) -> Action {
        if index >= self.queued.len() {
            return Action::None;
        }
        match button {
            QueueButton::Remove => {
                self.queued.remove(index);
            }
            QueueButton::SendNow => {
                let text = self.queued.remove(index);
                self.show_sent(&text);
                return Action::SendNow(text);
            }
            QueueButton::Edit if !self.input.text.is_empty() => {
                self.flash = Some("clear the box to edit a queued message".into());
            }
            QueueButton::Edit => {
                let text = self.queued.remove(index);
                self.input.set(text);
            }
        }
        Action::None
    }

    pub fn show_sent(&mut self, text: &str) {
        self.scroll = 0;
        self.transcript.push(Entry::User {
            text: text.to_owned(),
            open: false,
        });
    }
}
