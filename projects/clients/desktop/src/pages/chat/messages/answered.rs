//! The user's answer to the agent's questions, as one card: each question with what was
//! chosen under it.

use dusk_protocol::tools::Reply;
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::ui::Type;

pub fn answered_card(key: &str, replies: &[Reply], cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (muted, border, surface) = (theme.muted_foreground, theme.border, theme.secondary);
    let mut card = div()
        .id(SharedString::from(format!("answered-{key}")))
        .self_end()
        .w_full()
        .max_w(px(600.))
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .overflow_hidden();
    for (index, reply) in replies.iter().enumerate() {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_4()
                .py_2()
                .when(index > 0, |row| row.border_t_1().border_color(border))
                .child(div().body().child(reply.question.clone()))
                .child(div().small().text_color(muted).child(reply.answer.clone())),
        );
    }
    card.into_any_element()
}
