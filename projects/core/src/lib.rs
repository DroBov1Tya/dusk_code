//! dusk_core: all application logic. Front ends (CLI, TUI, GUI) are thin clients over it.
//!
//! The tree follows the subsystems, as a kernel does:
//! - `init`: paths, logs and the first start of the config folder;
//! - `config`: the settings file and its live copy;
//! - `catalog`: what the user edits as files: skills, agents and mode prompts;
//! - `release`: shipped resources, their sync into the config, and updates;
//! - `providers`, `mcp`, `tools`: what the agents talk to and act with;
//! - `runtime`, `chat`, `state`: running threads, client commands and the records;
//! - `util`: small helpers with no domain.

pub mod catalog;
pub mod chat;
pub mod config;
pub mod init;
pub mod mcp;
pub mod providers;
pub mod release;
pub mod runtime;
pub mod state;
pub mod tools;
pub mod util;

use std::{future::Future, pin::Pin};

/// A boxed future, so a trait with async methods can be used as `dyn Trait`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

#[cfg(test)]
mod testing;
