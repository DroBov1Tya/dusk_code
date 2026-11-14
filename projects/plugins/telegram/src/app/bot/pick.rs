//! A menu of options, two a row. A pick takes the buttons away, stays under the menu,
//! and the command it stands for goes to the client.

use teloxide::{
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
};

use super::{
    callback::Callback,
    runner::{HandlerResult, MyBot},
};
use crate::app::{
    bridge::{BotCommand, BotEvent, FromBot},
    context::Shared,
};

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

pub async fn press(bot: &MyBot, shared: &Shared, chat: ChatId, index: usize) -> HandlerResult {
    let Some(menu) = shared.menu.lock().take() else {
        return Ok(());
    };
    let Some(option) = menu.options.get(index).cloned() else {
        *shared.menu.lock() = Some(menu);
        return Ok(());
    };
    let _ = bot.edit_message_reply_markup(chat, menu.message).await;
    bot.send_message(chat, format!("\u{2192} {option}")).await?;
    if let Some(command) = BotCommand::picked(&menu.command, option) {
        shared.emit(BotEvent::Input(FromBot::Command(command)));
    }
    Ok(())
}
