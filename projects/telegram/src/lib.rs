//! A Telegram bot that mirrors one duskcode chat for its owner. What they write goes to
//! the chat; the agent's final answers, its questions and its approval requests come
//! back. The token lives only in memory, and the bot answers only the one Telegram user
//! who paired with it by the code the client shows.

mod bridge;
mod config;
mod format;
mod handlers;
mod outbox;
mod runner;

pub use bridge::{BotCommand, BotEvent, FromBot, ToBot};
pub use runner::TelegramBot;

/// Checks the token with Telegram and makes the bot ready to start; nothing is sent
/// until [`TelegramBot::start`].
pub async fn connect(token: String, id: usize) -> anyhow::Result<TelegramBot> {
    TelegramBot::connect(token, id).await
}
