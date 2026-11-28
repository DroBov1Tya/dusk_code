//! Messages written while the agent works. Each waits above the composer and stays
//! editable; when the turn ends they go as one message, and "Now" interrupts the turn.
//! One sent while the context is compacted goes to the core at once, which takes it in
//! when the summary is written; it shows here until then. Up over the empty composer, or
//! Edit, takes the newest back into the composer and it is not sent.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use dusk_protocol::thread::ThreadEvent;

use super::composer::{Change, Draft};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// A message waiting for its chat's turn to end.
pub struct Pending {
    pub thread_id: String,
    pub text: Entity<InputState>,
    /// What the composer held, attachments too, given back on edit while the text is
    /// as it was queued.
    pub draft: Option<Draft>,
    /// Already with the core, which takes it in once its compaction is done.
    pub sent: bool,
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let shown = ws.shown.as_deref()?;
    let count = ws
        .pending
        .iter()
        .filter(|pending| pending.thread_id == shown)
        .count();
    if count == 0 {
        return None;
    }
    let (background, muted, faint) = {
        let theme = cx.theme();
        (theme.background, theme.muted, theme.muted_foreground)
    };
    let mut list = div()
        .id("queued-prompts")
        .max_h(px(152.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();
    let newest_sent = ws
        .pending
        .iter()
        .rposition(|pending| pending.thread_id == shown && pending.sent);
    for (index, pending) in ws.pending.iter().enumerate() {
        if pending.thread_id != shown {
            continue;
        }
        if pending.sent {
            list = list.child(sent_row(ws, index, newest_sent == Some(index), cx));
            continue;
        }
        list = list.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_lg()
                .bg(background)
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&pending.text)
                            .aria_label("Queued message")
                            .small(),
                    ),
                )
                .child(
                    Button::new(("pending-edit", index))
                        .ghost()
                        .small()
                        .label("Edit")
                        .tooltip("Back into the composer; it is not sent")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.edit_pending(index, cx))),
                )
                .child(
                    Button::new(("pending-now", index))
                        .ghost()
                        .small()
                        .label("Send now")
                        .tooltip("Stop the turn and send this")
                        .on_click(
                            cx.listener(move |ws, _, _, cx| ws.send_pending(index, true, cx)),
                        ),
                )
                .child(
                    Button::new(("pending-drop", index))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Remove")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if index < ws.pending.len() {
                                ws.pending.remove(index);
                            }
                            cx.notify();
                        })),
                ),
        );
    }
    Some(
        div().pb_2().child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_lg()
                .bg(muted)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .group_name()
                        .child(widgets::icon(IconName::Inbox, faint))
                        .child(format!("{count} queued")),
                )
                .child(list),
        ),
    )
}

/// A message the core holds until its compaction is done: its text, and Edit on the newest,
/// the one the core gives back.
fn sent_row(ws: &Workspace, index: usize, newest: bool, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let text = ws.pending[index].text.read(cx).value().to_string();
    let edit = newest.then(|| {
        Button::new(("pending-edit", index))
            .ghost()
            .small()
            .label("Edit")
            .tooltip("Back into the composer; it is not sent")
            .on_click(cx.listener(move |ws, _, _, cx| ws.edit_pending(index, cx)))
    });
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_lg()
        .bg(theme.background)
        .child(div().flex_1().min_w_0().truncate().text_sm().child(text))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("after compacting"),
        )
        .children(edit)
}

impl Workspace {
    /// Keeps the text for after the shown chat's turn; `draft` is what the composer held.
    pub fn queue(
        &mut self,
        thread_id: String,
        text: String,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hold(thread_id, text, Some(draft), false, window, cx);
    }

    /// Shows a message the core holds while it compacts the chat's context.
    pub fn held_by_core(
        &mut self,
        thread_id: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hold(thread_id, text, None, true, window, cx);
    }

    fn hold(
        &mut self,
        thread_id: String,
        text: String,
        draft: Option<Draft>,
        sent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(text, window, cx);
            state
        });
        self.pending.push(Pending {
            thread_id,
            text: state,
            draft,
            sent,
        });
        cx.notify();
    }

    /// The core took in a message it held, oldest first, or has nothing held left.
    pub fn core_took_in(&mut self, thread_id: &str, all: bool) {
        let mut first = true;
        self.pending.retain(|pending| {
            let mine = pending.thread_id == thread_id && pending.sent;
            let gone = mine && (all || first);
            first &= !mine;
            !gone
        });
    }

    /// What the core says of the messages it holds: one taken in shows in the chat, a
    /// command runs or is refused, and nothing is held once the chat rests.
    pub fn held_heard(&mut self, thread_id: &str, event: &ThreadEvent) {
        match event {
            ThreadEvent::UserMessage(_) | ThreadEvent::Rejected { .. } => {
                self.core_took_in(thread_id, false)
            }
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle | ThreadEvent::Failed(_) => {
                self.core_took_in(thread_id, true)
            }
            _ => {}
        }
    }

    /// Up over the empty composer: the shown chat's newest waiting message goes back into
    /// it. Says whether there was one.
    pub fn edit_last_pending(&mut self, cx: &mut Context<Self>) -> bool {
        let shown = self.shown.as_deref();
        let Some(index) = self
            .pending
            .iter()
            .rposition(|pending| Some(pending.thread_id.as_str()) == shown)
        else {
            return false;
        };
        self.edit_pending(index, cx);
        true
    }

    /// Takes a waiting message back into its chat's composer, so it is not sent. One the
    /// core holds is asked back from it; it may have been taken in meanwhile.
    pub fn edit_pending(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.get(index) else {
            return;
        };
        let thread_id = pending.thread_id.clone();
        if !pending.sent {
            let pending = self.pending.remove(index);
            let text = pending.text.read(cx).value().to_string();
            let queued = pending.draft.as_ref().map(|draft| draft.flat());
            let draft = match pending.draft {
                // Edited in the row: its text is what the user wants back.
                Some(draft) if queued.as_deref() == Some(text.trim()) => draft,
                _ => Draft {
                    text,
                    ..Default::default()
                },
            };
            self.edit_draft(Some(thread_id), Change::Restore(draft), cx);
            return;
        }
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().unqueue(&id).await },
            move |ws, result, cx| match result {
                Ok(Some(text)) => {
                    // The core gave back its newest: that is the last row it holds.
                    if let Some(at) = ws
                        .pending
                        .iter()
                        .rposition(|pending| pending.thread_id == thread_id && pending.sent)
                    {
                        ws.pending.remove(at);
                    }
                    let draft = Draft {
                        text,
                        ..Default::default()
                    };
                    ws.edit_draft(Some(thread_id), Change::Restore(draft), cx);
                }
                // Taken in already: it shows in the chat.
                Ok(None) => {
                    ws.core_took_in(&thread_id, true);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Sends what waits for the chat as one message when its turn ends, as the TUI does.
    pub fn send_next_pending(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let (mine, rest) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition::<Vec<_>, _>(|pending| pending.thread_id == thread_id && !pending.sent);
        self.pending = rest;
        let text = mine
            .iter()
            .map(|pending| pending.text.read(cx).value().trim().to_owned())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        self.send_text(thread_id.to_owned(), text, false, cx);
    }

    /// Sends one waiting message; `now` interrupts the running turn.
    pub fn send_pending(&mut self, index: usize, now: bool, cx: &mut Context<Self>) {
        if index >= self.pending.len() {
            return;
        }
        let pending = self.pending.remove(index);
        let text = pending.text.read(cx).value().trim().to_owned();
        self.send_text(pending.thread_id, text, now, cx);
    }

    /// Sends the chat's last message once more, after a call that failed.
    pub fn send_again(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(text) = self
            .chats
            .get(thread_id)
            .and_then(|chat| chat.last_sent())
            .map(str::to_owned)
        else {
            return;
        };
        self.send_text(thread_id.to_owned(), text, false, cx);
    }

    fn send_text(&mut self, thread_id: String, text: String, now: bool, cx: &mut Context<Self>) {
        cx.notify();
        if text.is_empty() {
            return;
        }
        let (origin, retry) = (thread_id.clone(), text.clone());
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, now).await },
            move |ws, result, cx| {
                if let Err(error) = result {
                    ws.edit_draft(
                        Some(origin),
                        super::composer::Change::Restore(super::composer::Draft {
                            text: retry,
                            ..Default::default()
                        }),
                        cx,
                    );
                    ws.fail(error, cx);
                }
            },
        );
    }
}
