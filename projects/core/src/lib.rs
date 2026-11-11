//! dusk_core: all application logic. Front ends (CLI, TUI, GUI) are thin clients over it:
//! they see only `client`, the protocol's core side, and `app`, what their process needs
//! to start. Everything else is private to the core.
//!
//! The tree follows the subsystems, as a kernel does:
//! - `platform`: the machine, found first: its system, its screen and its sandbox;
//! - `init`: paths, logs and the first start of the config folder;
//! - `config`: the settings file and its live copy;
//! - `catalog`: what the user edits as files: skills, agents and mode prompts;
//! - `release`: shipped resources, their sync into the config, and updates;
//! - `providers`, `mcp`, `tools`: what the agents talk to and act with;
//! - `runtime`, `message`, `state`: running threads, what messages carry, and the records;
//! - `project`: named folders, their git state and files, read only inside them;
//! - `plugins`: what the community adds without changing this code;
//! - `backup`: `/migrate` archives, their restore, and the guard of an uninstall;
//! - `util`: small helpers with no domain.

pub mod app;
mod backup;
mod catalog;
pub mod client;
mod config;
mod init;
mod mcp;
mod message;
mod platform;
mod plugins;
mod project;
mod providers;
mod release;
mod runtime;
mod secrets;
mod state;
mod tools;
mod util;
mod voice;

use std::{future::Future, pin::Pin};

/// A boxed future, so a trait with async methods can be used as `dyn Trait`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

#[cfg(test)]
mod testing;
