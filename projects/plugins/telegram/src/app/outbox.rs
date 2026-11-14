//! What the client hands the bot, sent to the owner in order; and while the agent works,
//! word that the bot is alive every few minutes.

use std::sync::{Arc, atomic::Ordering};

use teloxide::{
    prelude::*,
    types::{InputFile, ParseMode},
};
use tokio::{sync::mpsc, time::Instant};

use crate::{
    app::{
        bot::{handlers::buttons, runner::MyBot, utils::text},
        bridge::ToBot,
        context::{OpenMenu, OpenQuestion, Shared},
    },
    config,
};

pub async fn run(bot: MyBot, shared: Arc<Shared>, mut incoming: mpsc::UnboundedReceiver<ToBot>) {
    let first = Instant::now() + config::ALIVE_EVERY;
    let mut alive = tokio::time::interval_at(first, config::ALIVE_EVERY);
    loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else {
                    break;
                };
                if matches!(message, ToBot::Working(true)) {
                    alive.reset();
                }
                // A failed send is not retried; the next message may get through.
                let _ = deliver(&bot, &shared, message).await;
            }
            _ = alive.tick(), if shared.working() => {
                if let Some(owner) = shared.owner() {
                    let _ = bot.send_message(owner.chat, config::ALIVE).await;
                }
            }
        }
    }
}

async fn deliver(bot: &MyBot, shared: &Shared, message: ToBot) -> anyhow::Result<()> {
    if let ToBot::Working(working) = message {
        shared.working.store(working, Ordering::Relaxed);
        return Ok(());
    }
    let Some(owner) = shared.owner() else {
        return Ok(());
    };
    let chat = owner.chat;
    match message {
        ToBot::Answer(markdown) => {
            shared.working.store(false, Ordering::Relaxed);
            let mut pieces = text::messages(&markdown, config::MAX_CHUNK_CHARS);
            if pieces.len() > config::MAX_ANSWER_MESSAGES {
                pieces.truncate(config::MAX_ANSWER_MESSAGES);
                pieces.push((config::CUT.into(), config::CUT.into()));
            }
            for (html, plain) in pieces {
                // Telegram refuses HTML it cannot parse; the text then goes as it is.
                let sent = bot
                    .send_message(chat, html)
                    .parse_mode(ParseMode::Html)
                    .await;
                if sent.is_err() {
                    bot.send_message(chat, plain).await?;
                }
            }
        }
        ToBot::Question {
            question,
            options,
            multiple,
        } => {
            let hint = if multiple {
                config::MULTIPLE
            } else {
                config::SINGLE
            };
            let text = format!("{}\n\n{question}\n\n{hint}", config::QUESTION);
            let mut open = OpenQuestion {
                checked: vec![false; options.len()],
                options,
                multiple,
                message: teloxide::types::MessageId(0),
            };
            let sent = bot
                .send_message(chat, text)
                .reply_markup(buttons::question::keyboard(&shared.nonce, &open))
                .await?;
            open.message = sent.id;
            *shared.question.lock() = Some(open);
        }
        ToBot::Approval { id, text } => {
            // One message must carry the whole request, however long the command.
            let text: String = text.chars().take(config::MAX_CHUNK_CHARS).collect();
            bot.send_message(chat, format!("{}\n\n{text}", config::APPROVAL))
                .reply_markup(buttons::approval::keyboard(&shared.nonce, id))
                .await?;
        }
        ToBot::Reply(text) => {
            bot.send_message(chat, text).await?;
        }
        ToBot::Menu {
            command,
            title,
            options,
        } if options.len() <= config::MAX_MENU_BUTTONS => {
            let title: String = title.chars().take(config::MAX_CHUNK_CHARS).collect();
            let sent = bot
                .send_message(chat, title)
                .reply_markup(buttons::pick::keyboard(&shared.nonce, &options))
                .await?;
            *shared.menu.lock() = Some(OpenMenu {
                command,
                options,
                message: sent.id,
            });
        }
        ToBot::Menu {
            command,
            title,
            options,
        } => {
            let list: String = options.iter().map(|option| format!("\n{option}")).collect();
            let text = format!("{title}\n{list}\n\nSend /{command} <name> to pick one.");
            let text: String = text.chars().take(config::MAX_CHUNK_CHARS).collect();
            bot.send_message(chat, text).await?;
        }
        ToBot::Image {
            path,
            mime_type,
            caption,
        } => {
            // The core keeps its images without an extension; Telegram goes by the name.
            let name = if mime_type == "image/png" {
                "image.png"
            } else {
                "image.jpg"
            };
            let bytes = tokio::fs::read(&path).await?;
            let mut photo = bot.send_photo(chat, InputFile::memory(bytes).file_name(name));
            if let Some(caption) = caption {
                photo = photo.caption(
                    caption
                        .chars()
                        .take(config::MAX_CAPTION_CHARS)
                        .collect::<String>(),
                );
            }
            photo.await?;
        }
        ToBot::Working(_) => {}
    }
    Ok(())
}
