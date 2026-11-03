//! What the client hands the bot, sent to the owner in order; and while the agent works,
//! word that the bot is alive every few minutes.

use std::sync::{Arc, atomic::Ordering};

use teloxide::{
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode},
};
use tokio::{sync::mpsc, time::Instant};

use crate::{
    bridge::ToBot,
    config, format,
    runner::{MyBot, OpenQuestion, Shared},
};

/// Telegram shows about this much of a button.
const MAX_BUTTON_CHARS: usize = 60;

pub async fn run(bot: MyBot, shared: Arc<Shared>, mut incoming: mpsc::UnboundedReceiver<ToBot>) {
    let mut alive =
        tokio::time::interval_at(Instant::now() + config::ALIVE_EVERY, config::ALIVE_EVERY);
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
            let mut pieces = format::messages(&markdown, config::MAX_CHUNK_CHARS);
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
                .reply_markup(question_keyboard(shared, &open))
                .await?;
            open.message = sent.id;
            if let Ok(mut slot) = shared.question.lock() {
                *slot = Some(open);
            }
        }
        ToBot::Approval { id, text } => {
            let buttons = vec![vec![
                InlineKeyboardButton::callback(config::ALLOW, shared.button(&format!("y:{id}"))),
                InlineKeyboardButton::callback(config::DENY, shared.button(&format!("n:{id}"))),
            ]];
            // One message must carry the whole request, however long the command.
            let text: String = text.chars().take(config::MAX_CHUNK_CHARS).collect();
            bot.send_message(chat, format!("{}\n\n{text}", config::APPROVAL))
                .reply_markup(InlineKeyboardMarkup::new(buttons))
                .await?;
        }
        ToBot::Reply(text) => {
            bot.send_message(chat, text).await?;
        }
        ToBot::Working(_) => {}
    }
    Ok(())
}

/// One option a row; with several, a mark on the checked ones and Send last.
pub fn question_keyboard(shared: &Shared, question: &OpenQuestion) -> InlineKeyboardMarkup {
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
                    InlineKeyboardButton::callback(
                        format!("{mark}{label}"),
                        shared.button(&format!("t:{index}")),
                    )
                }
                false => {
                    InlineKeyboardButton::callback(label, shared.button(&format!("c:{index}")))
                }
            };
            vec![button]
        })
        .collect();
    if question.multiple {
        rows.push(vec![InlineKeyboardButton::callback(
            config::SEND,
            shared.button("s"),
        )]);
    }
    InlineKeyboardMarkup::new(rows)
}
