//! Allow and Deny under an approval request: the choice stays under the request and
//! goes to the client, which checks that the request still waits.

use teloxide::{
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
};

use super::{
    callback::Callback,
    runner::{HandlerResult, MyBot},
};
use crate::{
    app::{
        bridge::{BotEvent, FromBot},
        context::Shared,
    },
    config,
};

pub fn keyboard(nonce: &str, id: u64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[
        InlineKeyboardButton::callback(config::ALLOW, Callback::Allow(id).data(nonce)),
        InlineKeyboardButton::callback(config::DENY, Callback::Deny(id).data(nonce)),
    ]])
}

pub async fn answer(
    bot: &MyBot,
    shared: &Shared,
    query: &CallbackQuery,
    id: u64,
    allow: bool,
) -> HandlerResult {
    if let Some(message) = query.regular_message() {
        let mark = if allow { config::ALLOW } else { config::DENY };
        let text = format!("{}\n\n\u{2192} {mark}", message.text().unwrap_or_default());
        let _ = bot
            .edit_message_text(message.chat.id, message.id, text)
            .await;
    }
    shared.emit(BotEvent::Input(FromBot::Approval { id, allow }));
    Ok(())
}
