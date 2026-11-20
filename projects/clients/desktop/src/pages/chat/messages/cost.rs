//! What a turn spent, said on the line of its row.

use crate::ui::widgets;

/// The spend of a turn as words: tokens, and the cost when it is known. `dear`: the turn
/// cost more than twice the chat's usual one, and the words say so.
pub fn spend_label(spent: Option<(u64, u64)>, dear: bool) -> Option<String> {
    let (tokens, micro) = spent?;
    let mut text = format!("{} tokens", widgets::short(tokens));
    if micro > 0 {
        text.push_str(&format!(
            " \u{00B7} {}",
            dusk_protocol::model::Cost::dollars(micro)
        ));
    }
    if dear {
        text.push_str(" \u{00B7} dearer than usual");
    }
    Some(text)
}
