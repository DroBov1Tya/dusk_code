//! Messages written while the agent works. Each waits above the composer and stays
//! editable; when the turn ends they go as one message, and "Now" interrupts the turn.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// A message waiting for its chat's turn to end.
pub struct Pending {
    pub thread_id: String,
    pub text: Entity<InputState>,
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
    let theme = cx.theme();
    let mut list = div()
        .id("queued-prompts")
        .max_h(px(152.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();
    for (index, pending) in ws.pending.iter().enumerate() {
        if pending.thread_id != shown {
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
                .bg(theme.background)
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&pending.text)
                            .aria_label("Queued message")
                            .small(),
                    ),
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
                .bg(theme.muted)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .group_name()
                        .child(widgets::icon(IconName::Inbox, theme.muted_foreground))
                        .child(format!("{count} queued")),
                )
                .child(list),
        ),
    )
}

impl Workspace {
    /// Keeps the text for after the shown chat's turn.
    pub fn queue(
        &mut self,
        thread_id: String,
        text: String,
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
        });
        cx.notify();
    }

    /// Sends what waits for the chat as one message when its turn ends, as the TUI does.
    pub fn send_next_pending(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let (mine, rest) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition::<Vec<_>, _>(|pending| pending.thread_id == thread_id);
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

    fn send_text(&mut self, thread_id: String, text: String, now: bool, cx: &mut Context<Self>) {
        cx.notify();
        if text.is_empty() {
            return;
        }
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, now).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
