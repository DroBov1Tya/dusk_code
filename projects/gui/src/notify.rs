//! System notifications while the window is in the background: a chat finished or
//! failed its turn, or waits for an answer. A click brings the window and that chat
//! forward. Like the TUI, a turn rings once, and only the chat's own turns ring.

use std::collections::HashMap;

use dusk_protocol::{thread::ThreadEvent, work::TurnStatus};
use gpui_kit::*;

use crate::workspace::Workspace;

/// The turn a chat runs, and whether it has rung already.
#[derive(Default)]
pub struct Alerts(HashMap<String, (Option<String>, bool)>);

impl Workspace {
    pub fn notify_event(&mut self, thread_id: &str, event: &ThreadEvent, cx: &mut Context<Self>) {
        let (turn, alerted) = self.alerts.0.entry(thread_id.to_owned()).or_default();
        let body = match event {
            ThreadEvent::TurnStarted(started) => {
                *turn = Some(started.turn_id.clone());
                *alerted = false;
                return;
            }
            ThreadEvent::Idle => {
                (*turn, *alerted) = (None, false);
                return;
            }
            ThreadEvent::TurnFinished(finished)
                if turn.as_deref() == Some(finished.turn_id.as_str()) =>
            {
                *turn = None;
                let body = match finished.status {
                    TurnStatus::Completed => "Done",
                    TurnStatus::Failed => "Failed",
                    TurnStatus::Running | TurnStatus::Interrupted => return,
                };
                if std::mem::replace(alerted, true) {
                    return;
                }
                body
            }
            ThreadEvent::Failed(_) => {
                *turn = None;
                if std::mem::replace(alerted, true) {
                    return;
                }
                "Failed"
            }
            ThreadEvent::Approval(_) => "Waits for your approval",
            ThreadEvent::Question(_) => "Asks you a question",
            _ => return,
        };
        // The window in front shows it already.
        if !self.status.notifications || cx.active_window().is_some() {
            return;
        }
        let Some(chat) = self.chats.get(thread_id) else {
            return;
        };
        cx.show_system_notification(SystemNotification {
            tag: thread_id.to_owned().into(),
            title: chat
                .title
                .clone()
                .unwrap_or_else(|| "duskcode".into())
                .into(),
            body: body.into(),
            actions: Vec::new(),
        });
    }

    /// A click on a notification opens its chat in front.
    pub fn answer_notifications(cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        cx.on_system_notification_response(move |response, cx| {
            cx.activate(true);
            let thread_id = response.tag.to_string();
            let _ = this.update(cx, |ws, cx| ws.open_chat(&thread_id, cx));
        });
    }
}
