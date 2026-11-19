//! Sending what is typed: commands, messages, the queue and recall.

use dusk_protocol::client::{Command, parse};

use super::{Action, Activity, App, Connection, Panel, TabMove, Tone, schedule::ScheduleAction};

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
            "/overview" => return Action::Overview,
            "/schedules" => return Action::Schedule(ScheduleAction::Open),
            "/files" => return Action::ChangedFiles,
            "/link" => {
                return match &self.thread_id {
                    Some(id) => Action::Copy(dusk_protocol::thread::thread_link(id)),
                    None => {
                        self.flash = Some("No chat yet: send a message first".into());
                        Action::None
                    }
                };
            }
            "/diff" => return Action::Diff,
            "/me" => return Action::OpenMe,
            "/pins" => return Action::Pin(super::PinAsk::List),
            "/pin" => return Action::Pin(super::PinAsk::Last),
            "/plugins store" => {
                self.panel = Some(Panel::Workshop(Box::default()));
                return Action::LoadStore;
            }
            "/plugins" => {
                self.panel = Some(Panel::Plugins {
                    selected: 0,
                    entry: None,
                });
                return Action::None;
            }
            _ => {}
        }
        // A name after it, as a Workshop card in the chat says: the Workshop opens narrowed
        // to it, where Enter asks before anything is installed.
        if let Some(name) = trimmed.strip_prefix("/plugins store ") {
            let filter = name.trim().to_owned();
            let view = super::Workshop {
                filter,
                ..super::Workshop::default()
            };
            self.panel = Some(Panel::Workshop(Box::new(view)));
            return Action::LoadStore;
        }
        if trimmed == "/tab" || trimmed.starts_with("/tab ") {
            return match TabMove::parse(&trimmed["/tab".len()..]) {
                Some(step) => Action::Tab(step),
                None => {
                    self.flash =
                        Some("Use /tab, /tab 2, /tab next, /tab prev or /tab close".into());
                    Action::None
                }
            };
        }
        if let Some(name) = trimmed.strip_prefix("/snippet ") {
            return Action::OpenSnippet(name.trim().trim_start_matches('@').to_owned());
        }
        if let Some(spec) = trimmed.strip_prefix("/schedule ") {
            return match super::schedule::parse(spec, chrono::Local::now()) {
                Ok((repeat, prompt)) => Action::Schedule(ScheduleAction::Add { repeat, prompt }),
                Err(error) => {
                    self.flash = Some(error);
                    Action::None
                }
            };
        }
        if let Some(number) = trimmed.strip_prefix("/unpin ") {
            return match number.trim().parse::<usize>() {
                Ok(number) if number > 0 => Action::Pin(super::PinAsk::Unpin(number)),
                _ => {
                    self.flash = Some("Use /unpin N, N as /pins numbers them".into());
                    Action::None
                }
            };
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
        let command = parse(&text);
        if let Some(Ok(command)) = &command
            && !command.runs_in_thread()
        {
            return Action::Command(command.clone());
        }
        // A message written instead of answering the question declines it.
        let text = match self.poll.take() {
            // The questions behind it go unasked: the message is what the user wants now.
            Some(poll) if command.is_none() => {
                self.polls_waiting.clear();
                self.poll_answers.push(poll.question.declined(&text));
                std::mem::take(&mut self.poll_answers).join("\n\n")
            }
            poll => {
                self.poll = poll;
                text
            }
        };
        if let Some(Ok(Command::Fork { .. })) = &command {
            self.activity = Activity::Forking;
        }
        if command.is_none() {
            // What the user says makes the suggestion stale.
            self.suggestion.clear();
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

#[cfg(test)]
mod tests {
    use crate::app::{Action, Panel, test_app};

    /// What a Workshop card in the chat tells the user to type opens the Workshop
    /// narrowed to the name; installing still waits for Enter and its box.
    #[test]
    fn plugins_store_with_a_name_opens_the_workshop_narrowed_to_it() {
        let mut app = test_app();
        app.input.set("/plugins store  motion-kit ".into());
        assert!(matches!(app.submit(false), Action::LoadStore));
        let Some(Panel::Workshop(view)) = &app.panel else {
            panic!("the Workshop is open");
        };
        assert_eq!(view.filter, "motion-kit");
        assert!(view.confirm.is_none());

        app.input.set("/plugins store".into());
        assert!(matches!(app.submit(false), Action::LoadStore));
        assert!(matches!(&app.panel, Some(Panel::Workshop(view)) if view.filter.is_empty()));
    }
}
