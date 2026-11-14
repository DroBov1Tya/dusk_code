//! An option picked from a menu: the buttons go, the pick stays under the menu, and the
//! command it stands for goes to the client.

use teloxide::prelude::*;

use crate::app::{
    bot::runner::{HandlerResult, MyBot},
    bridge::{BotCommand, BotEvent, FromBot},
    context::Shared,
};

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
