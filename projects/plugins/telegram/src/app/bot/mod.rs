//! The bot's side of Telegram: polling and routing in `runner`, one file per thing the
//! owner sees: messages and the menu (`owner`), pairing, a question, an approval
//! request, a pick from a menu, and the agent's Markdown as Telegram HTML (`text`).
//! Button data is parsed in `callback` before any handler sees it.

pub mod approval;
pub mod callback;
pub mod owner;
pub mod pairing;
pub mod pick;
pub mod question;
pub mod runner;
pub mod text;
