//! dusk_server: the core for browsers. It serves the built web client, signs a browser in
//! with a one-run token and carries the protocol's requests, responses and events over a
//! WebSocket, so a browser sees the same chats as the GUI and the TUI.
//!
//! - `auth`: the token, the sessions behind the cookie and the limit on failed sign-ins;
//! - `guard`: what every request must satisfy (host, origin) and the headers every answer has;
//! - `files`: the bundle embedded in the binary, and the pictures and sounds of chats;
//! - `stream`: the frame pump the WebSocket and the local JSON lines share;
//! - `bridge`: one WebSocket stream per attached browser;
//! - `local`: the core shared by the apps of one user: its run folder, lock and key, the
//!   JSON lines transport of the local API and the rules for a core of another version;
//! - `server`: the routes, and starting and stopping.

mod auth;
mod bridge;
mod files;
mod guard;
pub mod local;
mod server;
pub mod stream;

pub use server::{Config, DEFAULT_PORT, WebServer, start};
