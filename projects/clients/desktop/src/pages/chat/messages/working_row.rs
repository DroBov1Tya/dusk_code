//! The last row of the transcript while a turn runs: the working figure and what the turn
//! does now, with how long it has run, or the model call being tried again.

use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::{timing, transcript::Transcript, working},
    ui::Type,
};

pub fn render(shown: &str, chat: &Transcript, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .meta()
        .text_color(theme.muted_foreground)
        .child(working::render(
            SharedString::from(format!("working-{shown}")),
            theme.primary,
            theme.foreground,
        ))
        .child(
            match (&chat.retry, chat.span(chat.activity_boundaries.len())) {
                (Some((retry, arrived)), _) => div()
                    .min_w_0()
                    .truncate()
                    .child(
                        retry
                            .line(u64::try_from(arrived.elapsed().as_millis()).unwrap_or(u64::MAX)),
                    )
                    .into_any_element(),
                (None, Some((began, None))) => format!(
                    "Working \u{00B7} {}",
                    timing::duration(dusk_protocol::system::now_ms() - began)
                )
                .into_any_element(),
                _ => "Working".into_any_element(),
            },
        )
        .into_any_element()
}
