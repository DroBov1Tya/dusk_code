//! The options of a menu, two a row.

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::app::bot::callback::Callback;

pub fn keyboard(nonce: &str, options: &[String]) -> InlineKeyboardMarkup {
    let buttons: Vec<InlineKeyboardButton> = options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            InlineKeyboardButton::callback(option.clone(), Callback::Pick(index).data(nonce))
        })
        .collect();
    InlineKeyboardMarkup::new(buttons.chunks(2).map(<[_]>::to_vec))
}
