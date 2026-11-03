//! Button presses under the bot's messages. Data is this run's nonce, `|`, then one of
//! `c:<option>` (choose), `t:<option>` (toggle), `s` (send the toggled ones), `y:<id>`
//! and `n:<id>` (answer an approval request). Only the owner's presses of this run's
//! buttons count.

use std::sync::Arc;

use teloxide::prelude::*;

use crate::{
    bridge::{BotEvent, FromBot},
    config, outbox,
    runner::{HandlerResult, MyBot, Shared},
};

pub async fn on_callback(bot: MyBot, query: CallbackQuery, shared: Arc<Shared>) -> HandlerResult {
    bot.answer_callback_query(query.id.clone()).await?;
    let Some(owner) = shared.owner().filter(|owner| owner.user == query.from.id) else {
        return Ok(());
    };
    let data = query.data.as_deref().unwrap_or_default();
    let Some(data) = data
        .split_once('|')
        .filter(|(nonce, _)| *nonce == shared.nonce)
        .map(|(_, data)| data)
    else {
        return Ok(());
    };
    let (kind, value) = data.split_once(':').unwrap_or((data, ""));
    match kind {
        "c" | "t" | "s" => {
            let index = value.parse::<usize>().ok();
            let Some(input) = press_option(&bot, &shared, owner.chat, kind, index).await? else {
                return Ok(());
            };
            shared.emit(BotEvent::Input(input));
        }
        "y" | "n" => {
            let Ok(id) = value.parse::<u64>() else {
                return Ok(());
            };
            if let Some(message) = query.regular_message() {
                let mark = if kind == "y" {
                    config::ALLOW
                } else {
                    config::DENY
                };
                let text = format!("{}\n\n\u{2192} {mark}", message.text().unwrap_or_default());
                let _ = bot.edit_message_text(owner.chat, message.id, text).await;
            }
            shared.emit(BotEvent::Input(FromBot::Approval {
                id,
                allow: kind == "y",
            }));
        }
        _ => {}
    }
    Ok(())
}

/// A single choice is sent at once; with several, presses toggle until Send.
async fn press_option(
    bot: &MyBot,
    shared: &Shared,
    chat: ChatId,
    kind: &str,
    index: Option<usize>,
) -> anyhow::Result<Option<FromBot>> {
    let (chosen, message, keyboard) = {
        let Ok(mut open) = shared.question.lock() else {
            return Ok(None);
        };
        let Some(question) = open.as_mut() else {
            return Ok(None);
        };
        match (kind, index) {
            ("c", Some(index)) if index < question.options.len() => {
                let picked = question.options[index].clone();
                (Some((vec![index], picked)), question.message, None)
            }
            ("t", Some(index)) if index < question.options.len() => {
                question.checked[index] ^= true;
                (
                    None,
                    question.message,
                    Some(outbox::question_keyboard(shared, question)),
                )
            }
            ("s", _) => {
                let chosen: Vec<usize> = (0..question.options.len())
                    .filter(|&at| question.checked[at])
                    .collect();
                if chosen.is_empty() {
                    return Ok(None);
                }
                let picked = chosen
                    .iter()
                    .map(|&at| question.options[at].as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                (Some((chosen, picked)), question.message, None)
            }
            _ => return Ok(None),
        }
    };
    if let Some(keyboard) = keyboard {
        let _ = bot
            .edit_message_reply_markup(chat, message)
            .reply_markup(keyboard)
            .await;
        return Ok(None);
    }
    let Some((chosen, picked)) = chosen else {
        return Ok(None);
    };
    if let Ok(mut open) = shared.question.lock() {
        *open = None;
    }
    // The buttons go; what was chosen stays under the question.
    let _ = bot.edit_message_reply_markup(chat, message).await;
    bot.send_message(chat, format!("\u{2192} {picked}")).await?;
    Ok(Some(FromBot::Chosen(chosen)))
}
