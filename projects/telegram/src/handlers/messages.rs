//! Messages: pairing for a stranger with the code, commands and text for the owner,
//! silence for anyone else.

use std::sync::{Arc, atomic::Ordering};

use teloxide::{
    prelude::*,
    types::{ChatKind, User},
    utils::command::BotCommands,
};
use tracing::info;

use crate::{
    bridge::{BotCommand, BotEvent, FromBot},
    config::{self, Command},
    runner::{HandlerResult, MyBot, Owner, Shared},
};

pub async fn on_message(bot: MyBot, message: Message, shared: Arc<Shared>) -> HandlerResult {
    let Some(user) = message.from.clone() else {
        return Ok(());
    };
    let owner = shared.owner();
    let Some(owner) = owner else {
        return pair(bot, message, shared, user).await;
    };
    if owner.user != user.id || owner.chat != message.chat.id {
        return Ok(());
    }
    let Some(text) = message.text() else {
        bot.send_message(owner.chat, "Only text reaches the chat for now.")
            .await?;
        return Ok(());
    };
    let input = match Command::parse(text, &shared.username) {
        Ok(Command::Start(_) | Command::Help) => {
            bot.send_message(owner.chat, Command::descriptions().to_string())
                .reply_markup(config::keyboard())
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
        // Anything else is the chat's, its own slash commands included.
        Err(_) => FromBot::Message(text.to_owned()),
    };
    shared.emit(BotEvent::Input(input));
    Ok(())
}

/// The first user to send `/start <code>` in a private chat, while the code is good,
/// owns the bot; other messages from strangers get no answer.
async fn pair(bot: MyBot, message: Message, shared: Arc<Shared>, user: User) -> HandlerResult {
    let private = matches!(message.chat.kind, ChatKind::Private(_));
    let code = message
        .text()
        .and_then(|text| text.strip_prefix("/start"))
        .map(str::trim);
    let fresh = shared.created.elapsed() <= config::PAIRING_WINDOW
        && shared.attempts.load(Ordering::Relaxed) < config::MAX_PAIRING_ATTEMPTS;
    let Some(code) = code.filter(|_| private && fresh && !user.is_bot) else {
        return Ok(());
    };
    if code != shared.code {
        shared.attempts.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    let name = match &user.username {
        Some(username) => format!("@{username}"),
        None => user.full_name(),
    };
    {
        let Ok(mut owner) = shared.owner.lock() else {
            return Ok(());
        };
        if owner.is_some() {
            return Ok(());
        }
        *owner = Some(Owner {
            user: user.id,
            chat: message.chat.id,
            name: name.clone(),
        });
    }
    info!("telegram bot {} paired", shared.username);
    let _ = bot.set_my_commands(Command::bot_commands()).await;
    bot.send_message(message.chat.id, config::PAIRED)
        .reply_markup(config::keyboard())
        .await?;
    shared.emit(BotEvent::Paired { user: name });
    Ok(())
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
        Command::Mode => BotCommand::Mode,
        Command::Model(name) => BotCommand::Model(given(name)),
        Command::Compact(text) => BotCommand::Compact(given(text)),
        Command::Goal(text) => BotCommand::Goal(given(text)),
        // Answered by the bot itself before it gets here.
        Command::Start(_) | Command::Help | Command::Alive => BotCommand::Status,
    }
}
