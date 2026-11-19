//! Plugins that bring their own interface: what a plugin may do, where its parts sit in
//! the window, the views it draws with the host's widgets and the messages it trades
//! with the core. A plugin is a separate process; nothing here lets it reach the core's
//! secrets.
//!
//! - `contract`: what a plugin that serves a whole domain says to the core, and back;
//! - `compat`: the systems and clients a plugin runs on;
//! - `cover_art`: the gradient a plugin without a cover is drawn on;
//! - `permission`: what a plugin asks to be allowed, and nothing more is;
//! - `place`: where a plugin's part shows and what a click on it opens;
//! - `store`: the plugins the store offers and what a client asks of it;
//! - `view`: the widget tree a part is drawn from, and the events it sends back;
//! - `wire`: the messages between the core and the plugin's process.

mod compat;
pub mod contract;
mod cover_art;
mod permission;
mod place;
mod store;
mod view;
mod wire;

pub use compat::{Client, Os, allows, runs_here};
pub use contract::{ForgeProvides, Provides};
pub use cover_art::{CoverArt, Hsl, cover_art};
pub use permission::{KeyUse, Permissions, ProjectAccess};
pub use place::{Opens, Part, Place};
pub use store::{
    CATEGORIES, Connector, ConnectorState, Contents, Cover, FilePreview, FileRef, Kind,
    LOCAL_SOURCE, Listing, OFFICIAL_SOURCE, Order, PICKS, PREVIEW_BYTES, Picks, Probe, RepoStars,
    Runs, ScreenshotRef, SourceFailure, SourceRow, StoreList, StoreRequest, Tier, UpdateReport,
};
pub use view::{
    ButtonIcon, ListItem, MAX_NODES, MAX_TEXT, RowLayout, Size, Tone, TouchPhase, UiEvent, Widget,
    check_view,
};
pub use wire::{FromPlugin, MAX_FRAME, MAX_LINE, ToPlugin};
