//! Pairing: the first user to send `/start <code>` in a private chat, while the code is
//! good, owns the bot. A few wrong codes spend it; strangers get no answer.

use std::sync::{Arc, atomic::Ordering};

use teloxide::{
    prelude::*,
    types::{ChatKind, User},
    utils::command::BotCommands,
};
use tracing::info;

use crate::{
    app::{
        bot::{
            handlers::buttons,
            runner::{HandlerResult, MyBot},
        },
        bridge::BotEvent,
        context::{Owner, Shared},
    },
    config::{self, Command},
};

pub async fn pair(bot: MyBot, message: Message, shared: Arc<Shared>, user: User) -> HandlerResult {
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
        let mut owner = shared.owner.lock();
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
        .reply_markup(buttons::menu::keyboard())
        .await?;
    shared.emit(BotEvent::Paired { user: name });
    Ok(())
}
