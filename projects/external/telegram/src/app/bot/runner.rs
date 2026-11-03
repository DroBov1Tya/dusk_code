//! Polling one bot: the dispatcher with its handlers, spawned on the runtime, and the
//! routing of button presses to their handlers.

use std::sync::{Arc, atomic::Ordering};

use teloxide::{
    adaptors::throttle::Throttle,
    dispatching::{ShutdownToken, UpdateHandler},
    prelude::*,
};
use tokio::task::JoinHandle;

use super::{
    callback::Callback,
    handlers::{callbacks, commands},
};
use crate::app::{bridge::BotEvent, context::Shared};

pub type MyBot = Throttle<Bot>;
pub type HandlerResult = Result<(), anyhow::Error>;

/// Starts polling; the token stops it after the updates in hand.
pub fn start_bot(bot: MyBot, shared: Arc<Shared>) -> (JoinHandle<()>, ShutdownToken) {
    let mut dispatcher = Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![Arc::clone(&shared)])
        .build();
    let token = dispatcher.shutdown_token();
    let polling = tokio::spawn(async move {
        dispatcher.dispatch().await;
        if !shared.stopping.load(Ordering::Relaxed) {
            shared.emit(BotEvent::Stopped {
                reason: "polling ended".into(),
            });
        }
    });
    (polling, token)
}

fn schema() -> UpdateHandler<anyhow::Error> {
    dptree::entry()
        .branch(Update::filter_message().endpoint(commands::owner::on_message))
        .branch(Update::filter_callback_query().endpoint(route_callback))
}

/// Only the owner's presses of this run's buttons count.
async fn route_callback(bot: MyBot, query: CallbackQuery, shared: Arc<Shared>) -> HandlerResult {
    bot.answer_callback_query(query.id.clone()).await?;
    let Some(owner) = shared.owner().filter(|owner| owner.user == query.from.id) else {
        return Ok(());
    };
    let data = query.data.as_deref().unwrap_or_default();
    let Some(callback) = Callback::parse(data, &shared.nonce) else {
        return Ok(());
    };
    match callback {
        Callback::Choose(_) | Callback::Toggle(_) | Callback::Send => {
            callbacks::question::press(&bot, &shared, owner.chat, callback).await
        }
        Callback::Allow(id) => callbacks::approval::answer(&bot, &shared, &query, id, true).await,
        Callback::Deny(id) => callbacks::approval::answer(&bot, &shared, &query, id, false).await,
    }
}
