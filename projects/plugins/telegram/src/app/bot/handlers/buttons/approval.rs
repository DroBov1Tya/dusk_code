//! Allow and Deny under an approval request.

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::{app::bot::callback::Callback, config};

pub fn keyboard(nonce: &str, id: u64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[
        InlineKeyboardButton::callback(config::ALLOW, Callback::Allow(id).data(nonce)),
        InlineKeyboardButton::callback(config::DENY, Callback::Deny(id).data(nonce)),
    ]])
}
