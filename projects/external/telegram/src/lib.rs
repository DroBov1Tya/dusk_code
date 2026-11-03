//! A Telegram bot that mirrors one duskcode chat for its owner. What they write goes to
//! the chat; the agent's final answers, its questions and its approval requests come
//! back. The token lives only in memory, and the bot answers only the one Telegram user
//! who paired with it by the code the client shows.
//!
//! Layers, from the outside in:
//! - `app` — the bot a client connects, starts and stops;
//! - `app::bot` — Telegram handlers, buttons and texts;
//! - `app::workers` — the outbox that sends what the client hands over.

pub mod app;
pub mod config;

pub use app::{
    TelegramBot,
    bridge::{BotCommand, BotEvent, FromBot, ToBot},
};
