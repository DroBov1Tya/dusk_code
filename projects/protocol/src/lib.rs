//! dusk_protocol: what the core and its clients say to each other.
//!
//! Plain data with serde, no runtime: the core sends these types, the TUI, a GUI or a
//! platform plugin reads them. The crate is a set of domains; each domain is a module whose
//! `mod.rs` is its public face and re-exports every item flat (`thread::Message`), while its
//! files and submodules are how it is built:
//! - `client`: the contract itself: the hello, `request/`, the response, the event, the status
//!   and the slash `command/`s;
//! - `thread`: what happens in a thread: its events, `message/`s, turns, tasks, tokens, goal;
//! - `model`: what a model streams and costs, its tool calls and the providers;
//! - `tools`: what tools hand back and ask for: results, approvals, questions, cards;
//! - `agent`: an agent's phase, zone and what it does;
//! - `config`: what the user configures: policies, memory, providers, secrets, speech, MCP,
//!   plugins, the profile;
//! - `catalog`: skills and agents as files, and their problems;
//! - `project`: projects, their git state, diffs and files;
//! - `release`, `system`: releases and installs, and what the process takes.
//!
//! A new type goes into the domain it belongs to, in a file of its own when it has its own
//! reason to change; a new domain needs a reason none of these has.

pub mod agent;
pub mod catalog;
pub mod client;
pub mod config;
pub mod model;
pub mod project;
pub mod release;
pub mod system;
pub mod thread;
pub mod tools;
