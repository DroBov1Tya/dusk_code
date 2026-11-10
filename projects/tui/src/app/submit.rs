//! Sending what is typed: commands, messages, the queue and recall.

use dusk_protocol::{command, command::Command};

use super::{Action, Activity, App, Connection, Panel, Tone};

impl App {
    /// Takes the typed text for sending; commands show as notices, messages as the user's.
    pub fn submit(&mut self, now: bool) -> Action {
        if self.input.is_empty() {
            return Action::None;
        }
        let text = self.input.take();
        self.recall = None;
        self.recall_draft = None;
        let trimmed = text.trim();
        match trimmed {
            "/connect" => {
                self.panel = Some(Panel::Connect(Connection::default()));
                return Action::None;
            }
            "/resend" => {
                self.panel = Some(Panel::Unsent { selected: 0 });
                return Action::None;
            }
            "/plan" => return Action::OpenPlan,
            "/plugins" => {
                self.panel = Some(Panel::Plugins {
                    selected: 0,
                    entry: None,
                });
                return Action::None;
            }
            _ => {}
        }
        if let Some(name) = trimmed.strip_prefix("/disconnect ") {
            return Action::Disconnect(name.trim().to_owned());
        }
        if trimmed == "/access" || trimmed.starts_with("/access ") {
            let name = trimmed["/access".len()..].trim();
            return Action::Access((!name.is_empty()).then(|| name.to_lowercase()));
        }
        // Connection secrets belong only in the masked form, never in recall/history.
        if trimmed.split_whitespace().next() == Some("/connect") {
            self.panel = Some(Panel::Connect(Connection::default()));
            self.flash = Some("Enter connection details in the form".into());
            return Action::None;
        }
        // The core says what a command is; ones that do not run in the thread come
        // back from it as something to show.
        let command = command::parse(&text);
        if let Some(Ok(command)) = &command
            && !command.runs_in_thread()
        {
            return Action::Command(command.clone());
        }
        // A message written instead of answering the question declines it.
        let text = match self.poll.take() {
            Some(poll) if command.is_none() => poll.question.declined(&text),
            poll => {
                self.poll = poll;
                text
            }
        };
        if let Some(Ok(Command::Fork { .. })) = &command {
            self.activity = Activity::Forking;
        }
        if command.is_some() {
            self.scroll = 0;
            self.transcript.notice(text.trim().to_owned(), Tone::Info);
        } else if self.is_busy() && !now {
            self.queued.push(text);
            return Action::None;
        } else {
            self.show_sent(&text);
        }
        if now {
            Action::SendNow(text)
        } else {
            Action::Send(text)
        }
    }

    /// Steps through what was sent before, newest first.
    pub fn recall(&mut self, older: bool) {
        let next = match (self.recall, older) {
            (None, true) => self.sent.len().checked_sub(1),
            (None, false) => None,
            (Some(at), true) => Some(at.saturating_sub(1)),
            (Some(at), false) => (at + 1 < self.sent.len()).then_some(at + 1),
        };
        if self.recall.is_none() && next.is_some() {
            self.recall_draft = Some(std::mem::take(&mut self.input));
        }
        self.recall = next;
        match next {
            Some(at) => self.input.set(self.sent[at].clone()),
            None => {
                if let Some(draft) = self.recall_draft.take() {
                    self.input = draft;
                }
            }
        }
    }

    pub fn after_compaction(&mut self) {
        self.activity = if self.turn_running {
            Activity::Thinking
        } else {
            Activity::Idle
        };
    }
}
