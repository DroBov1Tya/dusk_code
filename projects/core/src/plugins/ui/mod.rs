//! Plugins that draw parts of the window. A plugin with `ui:` in its manifest gets one
//! interface process, started when a client first opens one of its parts and stopped a
//! while after the last part is closed, or when the plugin is switched off. They trade
//! one JSON object per line: `ToPlugin` in, `FromPlugin` out (`dusk_protocol::plugin`).
//!
//! What it sends is data: every view is checked before a client sees it, every frame
//! must be a JPEG of bounded size, and each goes only to the clients that opened that
//! part, as a terminal's bytes go to its owner.
//!
//! - `icon`: a part's icon, checked;
//! - `host`: the running interfaces and who opened which part;
//! - `process`: one interface process and its lines.

mod host;
pub mod icon;
mod process;

pub use host::{UiHost, UiSpec};

#[cfg(test)]
mod tests;
