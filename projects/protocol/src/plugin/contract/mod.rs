//! Contracts: what the core and a plugin's contract program say to each other so that a
//! plugin can serve a whole domain, such as a forge, through typed JSON lines.
//!
//! - `wire`: the `call` the core sends, the `result` and `log` lines a program answers;
//! - `forge`: the forge contract's operations, arguments and answers;
//! - `provides`: what a manifest says it implements.

pub mod forge;
mod provides;
pub mod wire;

pub use provides::{ForgeProvides, Provides};
pub use wire::{Answer, Call, ContractError, ErrorKind, FromProgram};
