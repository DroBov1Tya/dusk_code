//! Options under a question: a single choice is sent at once; with several, presses
//! toggle marks until Send.

use teloxide::prelude::*;

use crate::app::{
    bot::{
        callback::Callback,
        handlers::buttons,
        runner::{HandlerResult, MyBot},
    },
    bridge::{BotEvent, FromBot},
    context::Shared,
};

pub async fn press(
    bot: &MyBot,
    shared: &Shared,
    chat: ChatId,
    callback: Callback,
) -> HandlerResult {
    let (chosen, message, keyboard) = {
        let mut open = shared.question.lock();
        let Some(question) = open.as_mut() else {
            return Ok(());
        };
        let count = question.options.len();
        match callback {
            Callback::Choose(index) if index < count && !question.multiple => {
                let picked = question.options[index].clone();
                (Some((vec![index], picked)), question.message, None)
            }
            Callback::Toggle(index) if index < count && question.multiple => {
                question.checked[index] ^= true;
                let keyboard = buttons::question::keyboard(&shared.nonce, question);
                (None, question.message, Some(keyboard))
            }
            Callback::Send if question.multiple => {
                let chosen: Vec<usize> = (0..count).filter(|&at| question.checked[at]).collect();
                if chosen.is_empty() {
                    return Ok(());
                }
                let picked = chosen
                    .iter()
                    .map(|&at| question.options[at].as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                (Some((chosen, picked)), question.message, None)
            }
            _ => return Ok(()),
        }
    };
    if let Some(keyboard) = keyboard {
        let _ = bot
            .edit_message_reply_markup(chat, message)
            .reply_markup(keyboard)
            .await;
        return Ok(());
    }
    let Some((chosen, picked)) = chosen else {
        return Ok(());
    };
    *shared.question.lock() = None;
    // The buttons go; what was chosen stays under the question.
    let _ = bot.edit_message_reply_markup(chat, message).await;
    bot.send_message(chat, format!("\u{2192} {picked}")).await?;
    shared.emit(BotEvent::Input(FromBot::Chosen(chosen)));
    Ok(())
}
