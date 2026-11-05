//! dusk_core: all application logic. Front ends (CLI, TUI, GUI) are thin clients over it.
//!
//! The tree follows the subsystems, as a kernel does:
//! - `platform`: the machine, found first: its system, its screen and its sandbox;
//! - `init`: paths, logs and the first start of the config folder;
//! - `config`: the settings file and its live copy;
//! - `catalog`: what the user edits as files: skills, agents and mode prompts;
//! - `release`: shipped resources, their sync into the config, and updates;
//! - `providers`, `mcp`, `tools`: what the agents talk to and act with;
//! - `runtime`, `message`, `state`: running threads, what messages carry, and the records;
//! - `plugins`: what the community adds without changing this code;
//! - `client`: the protocol's core side; clients talk to the core through it;
//! - `backup`: `/migrate` archives, their restore, and the guard of an uninstall;
//! - `util`: small helpers with no domain.

pub mod backup;
pub mod catalog;
pub mod client;
pub mod config;
pub mod init;
pub mod mcp;
pub mod message;
pub mod platform;
pub mod plugins;
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
