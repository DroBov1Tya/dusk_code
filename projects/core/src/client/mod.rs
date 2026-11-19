//! The core's side of the protocol. `Hub` (in `hub`) is the running core that clients
//! attach to; `Client` (in `connection`) is one client's handle, with a part per area:
//! `catalog`, `plugins`, `projects`, `settings`, `terminal`, `threads` and `work`.
//! `serve` answers the calls inside the core, each one first checked against the `Scope`
//! (in `scope`) its client was attached with.

use anyhow::Result;
use dusk_protocol::client::Hello;

use crate::runtime::Runtime;

mod catalog;
mod connection;
mod hub;
mod plugins;
mod projects;
mod scope;
mod serve;
mod settings;
mod terminal;
mod threads;
mod work;

use connection::unexpected;

pub(crate) use serve::ClientId;

pub use catalog::Catalog;
pub use connection::Client;
pub use hub::Hub;
pub use plugins::Plugins;
pub use projects::Projects;
pub use scope::{Area, Scope};
pub use settings::Settings;
pub use terminal::Terminal;
pub use threads::Threads;
pub use work::Work;

/// How the web server reads a card by its id, by the core's own rule.
pub use crate::state::artifacts::ArtifactStore;

/// Starts the core's side of the protocol over `runtime` for the one client that said
/// `hello`, with `scope`. The core stops when that client goes away.
pub fn serve(runtime: Runtime, hello: Hello, scope: Scope) -> Result<Client> {
    Hub::start(runtime).attach(hello, scope)
}
