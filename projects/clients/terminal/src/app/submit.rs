//! Sending what is typed: commands, messages, the queue and recall.

use dusk_protocol::{
    client::{Command, parse},
    config::ModelTier,
};

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
            "/plans" => return Action::OpenPlans,
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
        if trimmed == "/tier" || trimmed.starts_with("/tier ") {
            return match parse_tier(&trimmed["/tier".len()..]) {
                Some(ask) => Action::Tier(ask),
                None => {
                    self.flash = Some(
                        "Use /tier, /tier strong|default|weak provider/model, or unset".into(),
                    );
                    Action::None
                }
            };
        }
        if trimmed == "/agents clear" {
            let count = self.clear_agents();
            self.flash = Some(match count {
                0 => "No finished subagents to clear".into(),
                count => format!(
                    "Cleared {}",
                    dusk_protocol::thread::plural(count as u64, "finished subagent")
                ),
            });
            return Action::None;
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
            Some(_) if command.is_none() => self.questions.decline(&text).unwrap_or(text),
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
        } else if self.is_busy() && !now && !self.compacting_before_turn() {
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

    /// The context is compacted before a turn: the core takes in what is sent now as
    /// soon as the summary is written, so it goes at once and shows as sent.
    fn compacting_before_turn(&self) -> bool {
        self.activity == Activity::Compacting && !self.turn_running
    }

    pub fn after_compaction(&mut self) {
        self.activity = if self.turn_running {
            Activity::Thinking
        } else {
            Activity::Idle
        };
    }
}

/// `/tier`'s arguments: none shows the tiers; a tier and `provider/model` sets it, a
/// tier and `unset` puts it back on the chat's model. `None` is a misspelling.
fn parse_tier(arguments: &str) -> Option<Option<(ModelTier, Option<String>)>> {
    let mut words = arguments.split_whitespace();
    let Some(tier) = words.next() else {
        return Some(None);
    };
    let tier = ModelTier::parse(&tier.to_lowercase())?;
    let model = match (words.next()?, words.next()) {
        ("unset", None) => None,
        (model, None) => Some(model.to_owned()),
        _ => return None,
    };
    Some(Some((tier, model)))
}

#[cfg(test)]
mod tests {
    use crate::app::{Action, Panel, test_app};
    use dusk_protocol::config::ModelTier;

    #[test]
    fn tier_sets_shows_or_unsets_a_subagent_model() {
        let mut app = test_app();
        app.input.set("/tier".into());
        assert!(matches!(app.submit(false), Action::Tier(None)));
        app.input.set("/tier Strong big/fable".into());
        assert!(matches!(
            app.submit(false),
            Action::Tier(Some((ModelTier::Strong, Some(model)))) if model == "big/fable"
        ));
        app.input.set("/tier weak unset".into());
        assert!(matches!(
            app.submit(false),
            Action::Tier(Some((ModelTier::Weak, None)))
        ));
        for wrong in ["/tier fast big/x", "/tier weak", "/tier weak a/b c"] {
            app.input.set(wrong.into());
            assert!(matches!(app.submit(false), Action::None), "{wrong}");
        }
    }

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
