//! The options of a question: one a row; with several, a mark on the checked ones and
//! Send last.

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::{
    app::{bot::callback::Callback, context::OpenQuestion},
    config,
};

/// Telegram shows about this much of a button.
const MAX_BUTTON_CHARS: usize = 60;

pub fn keyboard(nonce: &str, question: &OpenQuestion) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let label: String = option.chars().take(MAX_BUTTON_CHARS).collect();
            let button = match question.multiple {
                true => {
                    let mark = if question.checked[index] {
                        "\u{2713} "
                    } else {
                        ""
                    };
                    let data = Callback::Toggle(index).data(nonce);
                    InlineKeyboardButton::callback(format!("{mark}{label}"), data)
                }
                false => InlineKeyboardButton::callback(label, Callback::Choose(index).data(nonce)),
            };
            vec![button]
        })
        .collect();
    if question.multiple {
        let send = InlineKeyboardButton::callback(config::SEND, Callback::Send.data(nonce));
        rows.push(vec![send]);
    }
    InlineKeyboardMarkup::new(rows)
}
