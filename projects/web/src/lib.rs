//! dusk_web: the core for browsers. It serves the built web client, signs a browser in
//! with a one-run token and carries the protocol's requests, responses and events over a
//! WebSocket, so a browser sees the same chats as the GUI and the TUI.
//!
//! - `auth`: the token, the sessions behind the cookie and the limit on failed sign-ins;
//! - `guard`: what every request must satisfy (host, origin) and the headers every answer has;
//! - `assets`: the bundle embedded in the binary;
//! - `bridge`: one WebSocket stream per attached browser;
//! - `server`: the routes, and starting and stopping.

mod assets;
mod auth;
mod bridge;
mod guard;
mod server;

pub use server::{Config, WebServer, start};
