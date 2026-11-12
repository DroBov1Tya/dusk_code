//! What waits for the user: a tool's request for a yes, and a question of the agent.

use dusk_protocol::tools::{Decision, Request};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut cards = div()
        .flex()
        .flex_col()
        .gap_2()
        .px_6()
        .max_w(px(780.))
        .mx_auto()
        .w_full();
    let Some(chat) = ws.shown_chat() else {
        return cards;
    };
    let theme = cx.theme();
    let (border, warning, surface) = (theme.border, theme.warning, theme.muted);
    for request in &chat.approvals {
        let id = request.id;
        let mut buttons = div().flex().flex_row().gap_2().child(
            Button::new(SharedString::from(format!("once-{id}")))
                .primary()
                .small()
                .label("Allow once")
                .on_click(cx.listener(move |ws, _, _, cx| ws.approve(id, Decision::Once, cx))),
        );
        if let Some(scope) = &request.session {
            buttons = buttons.child(
                Button::new(SharedString::from(format!("session-{id}")))
                    .small()
                    .label(format!("Allow {scope} for the session"))
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.approve(id, Decision::Session, cx)),
                    ),
            );
        }
        buttons = buttons.child(
            Button::new(SharedString::from(format!("deny-{id}")))
                .danger()
                .small()
                .label("Deny")
                .on_click(cx.listener(move |ws, _, _, cx| ws.approve(id, Decision::Deny, cx))),
        );
        // A command is shown as code, in full: it came from the model and is about to run.
        let what = match &request.request {
            Request::Command { command } => {
                let mono = crate::ui::theme::prefs(cx)
                    .mono_font
                    .clone()
                    .unwrap_or_else(|| "Menlo".to_owned());
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child("The agent wants to run")
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(cx.theme().background)
                            .small()
                            .font_family(mono)
                            .child(command.clone()),
                    )
            }
            other => div().child(other.describe("The agent")),
        };
        cards = cards.child(
            card(border, surface)
                .border_color(warning)
                .child(what)
                .child(buttons),
        );
    }
    if let Some(question) = &chat.question {
        let muted = cx.theme().muted_foreground;
        let (primary, hover) = (cx.theme().primary, muted.opacity(0.5));
        let mut options = div().flex().flex_col().gap_2();
        for (index, option) in question.options.iter().enumerate() {
            let picked = ws.picked.contains(&index);
            // A tick for what is chosen in a list of several, the number otherwise.
            let badge = if question.multiple && picked {
                widgets::icon(IconName::CircleCheck, primary).into_any_element()
            } else {
                div()
                    .flex_none()
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if picked { primary } else { border })
                    .meta()
                    .text_color(if picked { primary } else { muted })
                    .child((index + 1).to_string())
                    .into_any_element()
            };
            options = options.child(
                div()
                    .id(("option", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(if picked { primary } else { border })
                    .when(picked, |card| card.bg(primary.opacity(0.08)))
                    .cursor_pointer()
                    .hover(move |card| card.border_color(if picked { primary } else { hover }))
                    .child(badge)
                    .child(div().flex_1().min_w_0().child(option.clone()))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.pick(index, cx))),
            );
        }
        let mut card = card(border, surface)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(question.question.clone()),
            )
            .child(div().meta().text_color(muted).child(if question.multiple {
                "Pick any. Or write your own answer below."
            } else {
                "Pick one. Or write your own answer below."
            }))
            .child(options);
        if question.multiple {
            card = card.child(
                Button::new("answer-chosen")
                    .primary()
                    .label("Answer")
                    .on_click(cx.listener(|ws, _, _, cx| ws.answer_picked(cx))),
            );
        }
        cards = cards.child(card);
    }
    cards
}

fn card(border: Hsla, surface: Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
}

impl Workspace {
    fn approve(&mut self, id: u64, decision: Decision, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.threads().approve(id, decision).await },
            move |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
                // The core closes it with an event as well; this hides it at once.
                for chat in ws.chats.values_mut() {
                    chat.approvals.retain(|request| request.id != id);
                }
                cx.notify();
            },
        );
    }

    /// One choice answers at once; with several allowed, it is marked until Answer.
    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let multiple = self
            .shown_chat()
            .and_then(|chat| chat.question.as_ref())
            .is_some_and(|q| q.multiple);
        if !multiple {
            self.picked = vec![index];
            self.answer_picked(cx);
            return;
        }
        if let Some(at) = self.picked.iter().position(|&picked| picked == index) {
            self.picked.remove(at);
        } else {
            self.picked.push(index);
        }
        cx.notify();
    }

    fn answer_picked(&mut self, cx: &mut Context<Self>) {
        let chosen = std::mem::take(&mut self.picked);
        if chosen.is_empty() {
            return;
        }
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&thread_id) else {
            return;
        };
        let Some(question) = chat.question.take() else {
            return;
        };
        let text = question.answer(&chosen, "");
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, false).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}
