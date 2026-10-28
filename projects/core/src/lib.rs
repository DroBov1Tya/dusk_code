//! dusk_core: all application logic. Front ends (CLI, TUI, GUI) are thin clients over it.

pub mod api;
pub mod commands;
pub mod config;
pub mod graph;
pub mod init;
pub mod live_config;
pub mod paths;
pub mod providers;
pub mod runtime;
pub mod state;
pub mod thread_start;
pub mod tools;

use std::{future::Future, pin::Pin};

/// A boxed future, so a trait with async methods can be used as `dyn Trait`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

#[cfg(test)]
mod testing;
