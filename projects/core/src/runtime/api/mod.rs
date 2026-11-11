//! The runtime's calls, one file for each area a client asks about. Each file is an `impl Runtime`
//! block: the struct and its start are in `runtime.rs`.

mod agents;
mod context;
mod mcp;
mod model;
mod plugins;
mod providers;
mod secrets;
mod settings;
mod skills;
mod system;
mod threads;
mod updates;
mod voice;
mod work;
