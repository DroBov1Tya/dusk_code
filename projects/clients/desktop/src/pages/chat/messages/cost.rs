//! What a turn spent, beside its row.

use gpui_kit::{component::ActiveTheme, *};

use crate::ui::{Type, widgets};

/// The spend of a turn beside its row: tokens, and the cost when it is known; a turn
/// more than twice as dear as the chat's usual one says so in the warning color.
pub trait TurnCost {
    fn child_cost(self, spent: Option<(u64, u64)>, dear: bool, cx: &App) -> Self;
}

impl TurnCost for Div {
    fn child_cost(self, spent: Option<(u64, u64)>, dear: bool, cx: &App) -> Self {
        let Some((tokens, micro)) = spent else {
            return self;
        };
        let theme = cx.theme();
        let mut text = format!("{} tokens", widgets::short(tokens));
        if micro > 0 {
            text.push_str(&format!(
                " \u{00B7} {}",
                dusk_protocol::model::Cost::dollars(micro)
            ));
        }
        self.child(
            div()
                .pl(px(32.))
                .meta()
                .text_color(if dear {
                    theme.warning
                } else {
                    theme.muted_foreground
                })
                .child(if dear {
                    format!("{text} \u{00B7} dearer than usual")
                } else {
                    text
                }),
        )
    }
}
