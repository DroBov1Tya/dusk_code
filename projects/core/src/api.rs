//! Core version shared with front ends; the single source is the workspace `Cargo.toml`.

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
