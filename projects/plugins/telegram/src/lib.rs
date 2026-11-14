//! A Telegram bot for one duskcode thread, run by the core as the built-in `telegram`
//! plugin's platform. What its owner writes goes to the thread; the agent's final
//! answers, its questions and its approval requests come back. The bot answers only the
//! one Telegram user who paired with it by the code `/plugins` shows.
//!
//! Layers, from the outside in:
//! - `app` — the bot a client connects, starts and stops;
//! - `app::bot` — Telegram handlers, buttons and texts, one file per feature;
//! - `app::outbox` — sends the owner what the client hands over;
//! - `platform` — the bot spoken to by the core over the platform protocol.

pub mod app;
pub mod config;
pub mod platform;

pub use app::{
    TelegramBot,
    bridge::{BotCommand, BotEvent, FromBot, ToBot},
};
