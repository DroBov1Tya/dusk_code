//! Messages: commands and text from the owner, pairing for a stranger with the code,
//! silence for anyone else; and the menu of commands under the message box.

use std::sync::Arc;

use teloxide::{
    prelude::*,
    types::{KeyboardButton, KeyboardMarkup},
    utils::command::BotCommands,
};

use super::{
    pairing,
    runner::{HandlerResult, MyBot},
};
use crate::{
    app::{
        bridge::{BotCommand, BotEvent, FromBot},
        context::Shared,
    },
    config::{self, Command},
};

pub async fn on_message(bot: MyBot, message: Message, shared: Arc<Shared>) -> HandlerResult {
    let Some(user) = message.from.clone() else {
        return Ok(());
    };
    let Some(owner) = shared.owner() else {
        return pairing::pair(bot, message, shared, user).await;
    };
    if owner.user != user.id || owner.chat != message.chat.id {
        return Ok(());
    }
    let Some(text) = message.text() else {
        bot.send_message(owner.chat, config::ONLY_TEXT).await?;
        return Ok(());
    };
    let input = match Command::parse(text, &shared.username) {
        Ok(Command::Start(_) | Command::Help) => {
            bot.send_message(owner.chat, Command::descriptions().to_string())
                .reply_markup(menu())
                .await?;
            return Ok(());
        }
        Ok(Command::Alive) => {
            let text = match shared.working() {
                true => config::ALIVE,
                false => config::ALIVE_IDLE,
            };
            bot.send_message(owner.chat, text).await?;
            return Ok(());
        }
        Ok(command) => FromBot::Command(bridge_command(command)),
        // Anything else is the chat's; the client decides what to take.
        Err(_) => FromBot::Message(text.to_owned()),
    };
    shared.emit(BotEvent::Input(input));
    Ok(())
}

/// The buttons under the message box: the commands used most.
pub fn menu() -> KeyboardMarkup {
    let row = |labels: [&str; 2]| labels.map(KeyboardButton::new).to_vec();
    KeyboardMarkup::new([
        row(["/stop", "/resume"]),
        row(["/status", "/approve"]),
        row(["/mode", "/model"]),
    ])
    .resize_keyboard()
    .persistent()
}

fn bridge_command(command: Command) -> BotCommand {
    let given = |text: String| {
        let text = text.trim().to_owned();
        (!text.is_empty()).then_some(text)
    };
    match command {
        Command::Stop => BotCommand::Stop,
        Command::Resume => BotCommand::Resume,
        Command::Status => BotCommand::Status,
        Command::Approve => BotCommand::Approve,
        Command::Deny => BotCommand::Deny,
        Command::Mode(name) => BotCommand::Mode(given(name)),
        Command::Model(name) => BotCommand::Model(given(name)),
        Command::Compact(text) => BotCommand::Compact(given(text)),
        Command::Goal(text) => BotCommand::Goal(given(text)),
        // Answered by the bot itself before it gets here.
        Command::Start(_) | Command::Help | Command::Alive => BotCommand::Status,
    }
}
