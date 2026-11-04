//! Requests of tools waiting for the user's yes, shown above the message box ahead of
//! everything else, the oldest first. Enter answers with the row under the cursor, Esc
//! denies; the core closes a request whose call is gone.

use crossterm::event::{KeyCode, KeyEvent};
use dusk_core::tools::{ApprovalRequest, Decision, Request};

use super::{Action, App};

pub struct Asked {
    pub request: ApprovalRequest,
    /// The agent that asks; `None` for the chat itself.
    pub agent: Option<String>,
    pub cursor: usize,
}

impl Asked {
    /// The answers offered, in order; the session one only when the core can keep it.
    pub fn choices(&self) -> Vec<(Decision, String)> {
        let mut choices = vec![(Decision::Once, "allow once".to_owned())];
        if let Some(grant) = self.request.request.session_grant() {
            let what = match self.request.request {
                Request::Path { write: true, .. } => format!("allow writing in {grant}"),
                Request::Path { write: false, .. } => format!("allow reading in {grant}"),
                Request::Command { .. } => format!("allow `{grant}` commands"),
                Request::Screen { .. } => "allow the screen".into(),
                Request::SystemPointer { .. } => "allow control of the system pointer".into(),
                Request::Keyboard { .. } => String::new(),
            };
            choices.push((Decision::Session, format!("{what} for this session")));
        }
        choices.push((Decision::Deny, "deny".to_owned()));
        choices
    }

    /// What the tool asks for, in a line.
    pub fn title(&self) -> String {
        let who = self.agent.as_deref().unwrap_or("the agent");
        self.request.request.describe(who)
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
        let asked = self.approvals.remove(0);
        let choices = asked.choices();
        let at = choice.unwrap_or(asked.cursor).min(choices.len() - 1);
        Action::Approve {
            id: asked.request.id,
            decision: choices[at].0,
        }
    }
}
