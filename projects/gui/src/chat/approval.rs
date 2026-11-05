//! What waits for the user: a tool's request for a yes, and a question of the agent.

use dusk_protocol::tools::Decision;
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut cards = div().flex().flex_col().gap_2().px_6().max_w(px(900.)).mx_auto().w_full();
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
                    .on_click(cx.listener(move |ws, _, _, cx| ws.approve(id, Decision::Session, cx))),
            );
        }
        buttons = buttons.child(
            Button::new(SharedString::from(format!("deny-{id}")))
                .danger()
                .small()
                .label("Deny")
                .on_click(cx.listener(move |ws, _, _, cx| ws.approve(id, Decision::Deny, cx))),
        );
        cards = cards.child(
            card(border, surface)
                .border_color(warning)
                .child(request.request.describe("The agent"))
                .child(buttons),
        );
    }
    if let Some(question) = &chat.question {
        let mut options = div().flex().flex_row().flex_wrap().gap_2();
        for (index, option) in question.options.iter().enumerate() {
            options = options.child(
                Button::new(SharedString::from(format!("option-{index}")))
                    .small()
                    .label(option.clone())
                    .on_click(cx.listener(move |ws, _, _, cx| ws.answer(index, cx))),
            );
        }
        cards = cards.child(
            card(border, surface)
                .child(div().font_weight(FontWeight::MEDIUM).child(question.question.clone()))
                .child(options)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Or write your own answer below."),
                ),
        );
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

    fn answer(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&thread_id) else {
            return;
        };
        let Some(question) = chat.question.take() else {
            return;
        };
        let text = question.answer(&[index], "");
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
