//! Plugins: what the community adds to duskcode without changing its code. A plugin is a
//! folder under `rules/plugins/` with a `plugin.yml`; switched on under `plugins:` in the
//! config, it can bring skills and agents, MCP servers for tools, providers, hooks run
//! when something happens in a thread, and a platform such as a chat bot. Its programs
//! run as processes of their own, so no plugin runs inside the core.
//!
//! - `manifest` — what `plugin.yml` says;
//! - `catalog` — finding plugins and applying those that are on;
//! - `install` — their skills and agents in the config folder;
//! - `hooks` — programs run on thread events;
//! - `platform` — programs that bring the user in from elsewhere.

pub mod catalog;
pub mod hooks;
mod install;
pub mod manifest;
pub mod platform;

pub use catalog::{PluginInfo, Plugins};
pub use hooks::Hooks;
pub use manifest::HookEvent;
pub use platform::Platforms;
