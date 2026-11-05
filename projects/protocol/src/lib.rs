//! dusk_protocol: what the core and its clients say to each other.
//!
//! Plain data with serde, no runtime: the core sends these types, the TUI, a GUI or a
//! platform plugin reads them. Each module is one area of a thread:
//! - `thread`: the events a thread sends;
//! - `conversation`, `work`, `goal`, `agent`: what those events carry;
//! - `model`, `tools`, `artifact`: model output, tool calls and their pictures;
//! - `config`, `settings`, `providers`, `mcp`, `catalog`: what the settings show;
//! - `update`, `system`, `paths`: releases, resource use and path completion.

pub mod agent;
pub mod artifact;
pub mod catalog;
pub mod config;
pub mod conversation;
pub mod goal;
pub mod mcp;
pub mod model;
pub mod paths;
pub mod providers;
pub mod settings;
pub mod system;
pub mod thread;
pub mod tools;
pub mod update;
pub mod work;
