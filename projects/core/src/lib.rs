//! dusk_core: all application logic. Front ends (CLI, TUI, GUI) are thin clients over it.

pub mod api;
pub mod config;
pub mod events;
pub mod init;
pub mod live_config;
pub mod paths;
pub mod providers;
pub mod receivers;
pub mod runtime;
pub mod session;
pub mod state;
pub mod tools;

#[cfg(test)]
mod testing;
