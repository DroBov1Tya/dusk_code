//! Requests of tools waiting for the user's yes, shown above the message box ahead of
//! everything else, the oldest first. Enter answers with the row under the cursor, Esc
//! denies; the core closes a request whose call is gone.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use dusk_protocol::tools::{ApprovalRequest, Decision, Request};

use super::{Action, App};

pub struct Asked {
    pub request: ApprovalRequest,
    /// The agent that asks; `None` for the chat itself.
    pub agent: Option<String>,
    pub cursor: usize,
    /// The first row of a batch's steps in view, when they do not all fit.
    pub step_scroll: usize,
    /// Step rows in view on the last drawn frame.
    pub step_page: usize,
    /// When every step of a batch had first been on screen. Until then a yes pages on
    /// instead of answering, so no step is allowed unseen.
    pub steps_seen_at: Option<Instant>,
}

/// After the last steps of a batch first show, a yes waits this long, so a held or
/// repeated Enter that paged through does not also allow what it just showed.
pub const SETTLE: Duration = Duration::from_millis(600);

impl Asked {
    /// The answers offered, in order; the session one only when the core can keep it.
    pub fn choices(&self) -> Vec<(Decision, String)> {
        let mut choices = vec![(Decision::Once, "allow once".to_owned())];
        if let Some(grant) = &self.request.session {
            let what = match self.request.request {
                Request::Path { write: true, .. } => format!("allow writing in {grant}"),
                Request::Path { write: false, .. } => format!("allow reading in {grant}"),
                Request::Command { .. } => format!("allow `{grant}` commands"),
                Request::Screen { .. } => "allow the screen".into(),
                Request::SystemPointer { .. } => "allow control of the system pointer".into(),
                Request::Keyboard { .. } | Request::Settings { .. } => String::new(),
                Request::Window { .. } => format!("allow {grant}"),
                Request::Git { .. } => format!("allow {grant}"),
            };
            choices.push((Decision::Session, format!("{what} for this session")));
        }
        choices.push((Decision::Deny, "deny".to_owned()));
        choices
    }

    /// A yes to a batch waits until every step has been on screen for [`SETTLE`]; a no
    /// and a single request never wait.
    pub fn answers_now(&self, decision: Decision, now: Instant) -> bool {
        decision == Decision::Deny
            || self.request.steps.is_empty()
            || self
                .steps_seen_at
                .is_some_and(|at| now.saturating_duration_since(at) >= SETTLE)
    }

    /// What the tool asks for, in a line.
    pub fn title(&self) -> String {
        let who = self.agent.as_deref().unwrap_or("the agent");
        self.request.title(who)
    }
}

/// A row of the shown request on the last drawn frame.
pub struct ApprovalHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub choice: usize,
}

impl App {
    pub fn ask_approval(&mut self, request: ApprovalRequest, agent: Option<String>) {
        self.approvals.push(Asked {
            request,
            agent,
            cursor: 0,
            step_scroll: 0,
            step_page: 0,
            steps_seen_at: None,
        });
    }

    pub fn close_approval(&mut self, id: u64) {
        self.approvals.retain(|asked| asked.request.id != id);
    }

    pub fn approval_key(&mut self, key: KeyEvent) -> Action {
        let Some(asked) = self.approvals.first_mut() else {
            return Action::None;
        };
        let last = asked.choices().len() - 1;
        match key.code {
            KeyCode::Up => asked.cursor = asked.cursor.saturating_sub(1),
            KeyCode::Down => asked.cursor = (asked.cursor + 1).min(last),
            KeyCode::PageDown => asked.step_scroll += asked.step_page.max(1),
            KeyCode::PageUp => {
                asked.step_scroll = asked.step_scroll.saturating_sub(asked.step_page.max(1))
            }
            KeyCode::Enter => return self.answer_approval(None),
            KeyCode::Esc => return self.answer_approval(Some(last)),
            _ => {}
        }
        Action::None
    }

    /// Answers the shown request with `choice`, or with the row under the cursor.
    pub fn answer_approval(&mut self, choice: Option<usize>) -> Action {
        if self.approvals.is_empty() {
            return Action::None;
        }
        let asked = &mut self.approvals[0];
        let choices = asked.choices();
        let at = choice.unwrap_or(asked.cursor).min(choices.len() - 1);
        if !asked.answers_now(choices[at].0, Instant::now()) {
            // Still unseen steps: show them. Just shown: wait, as if not pressed.
            if asked.steps_seen_at.is_none() {
                asked.step_scroll += asked.step_page.max(1);
            }
            return Action::None;
        }
        let asked = self.approvals.remove(0);
        Action::Approve {
            thread_id: self.thread_id.clone().unwrap_or_default(),
            id: asked.request.id,
            decision: choices[at].0,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dusk_protocol::tools::ApprovalStep;

    /// A batch of `count` keyboard steps, each typing `text`.
    pub(crate) fn batch(count: usize, text: &str) -> Asked {
        let steps: Vec<ApprovalStep> = (1..=count)
            .map(|number| ApprovalStep {
                number,
                request: Request::Keyboard {
                    action: format!("type \"{text}\""),
                },
                allowed: false,
            })
            .collect();
        Asked {
            request: ApprovalRequest {
                id: 1,
                request: Request::Keyboard {
                    action: format!("{count} computer actions"),
                },
                session: None,
                steps,
            },
            agent: None,
            cursor: 0,
            step_scroll: 0,
            step_page: 0,
            steps_seen_at: None,
        }
    }

    #[test]
    fn a_batch_is_allowed_only_a_while_after_every_step_was_shown() {
        let now = Instant::now();
        let mut asked = batch(20, "hi");
        assert!(!asked.answers_now(Decision::Once, now));
        assert!(!asked.answers_now(Decision::Session, now));
        assert!(asked.answers_now(Decision::Deny, now));
        asked.steps_seen_at = Some(now);
        // A held Enter that showed the last page does not also allow it.
        assert!(!asked.answers_now(Decision::Once, now + SETTLE / 2));
        assert!(asked.answers_now(Decision::Deny, now));
        assert!(asked.answers_now(Decision::Once, now + SETTLE));
        assert!(batch(0, "").answers_now(Decision::Once, now));
    }
}
