//! The core's side of the protocol. `Hub` (in `hub`) is the running core that clients
//! attach to; `Client` (in `connection`) is one client's handle, with a part per area:
//! `catalog`, `projects`, `settings`, `simulator`, `terminal`, `threads` and `work`.
//! `serve` answers the calls inside the core.

use anyhow::Result;
use dusk_protocol::client::Hello;

use crate::runtime::Runtime;

mod catalog;
mod connection;
mod hub;
mod plugins;
mod projects;
mod serve;
mod settings;
mod simulator;
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
pub use settings::Settings;
pub use simulator::Simulator;
pub use terminal::Terminal;
pub use threads::Threads;
pub use work::Work;

/// Starts the core's side of the protocol over `runtime` for the one client that said
/// `hello`. The core stops when that client goes away.
pub fn serve(runtime: Runtime, hello: Hello) -> Result<Client> {
    Hub::start(runtime).attach(hello)
}
