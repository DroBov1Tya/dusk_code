//! dusk_protocol: what the core and its clients say to each other.
//!
//! Plain data with serde, no runtime: the core sends these types, the TUI, a GUI or a
//! platform plugin reads them. Each module is one area of a thread:
//! - `client`: requests, responses, events and the status, the contract itself;
//! - `thread`: the events a thread sends;
//! - `conversation`, `work`, `goal`, `agent`: what those events carry;
//! - `model`, `tools`, `artifact`: model output, tool calls and their pictures;
//! - `config`, `settings`, `providers`, `mcp`, `catalog`: what the settings show;
//! - `project`: projects, their git state and files;
//! - `update`, `system`, `paths`: releases, resource use and path completion;
//! - `pasted`: how pasted text and sources are marked inside a message;
//! - `mentions`: the profile and snippets a message names with `@`.

pub mod agent;
pub mod artifact;
pub mod catalog;
pub mod client;
pub mod command;
pub mod config;
pub mod context;
pub mod conversation;
pub mod goal;
pub mod mcp;
pub mod mentions;
pub mod model;
pub mod pasted;
pub mod paths;
pub mod plugins;
pub mod project;
pub mod providers;
pub mod secrets;
pub mod settings;
pub mod system;
pub mod thread;
pub mod tools;
pub mod update;
pub mod work;
