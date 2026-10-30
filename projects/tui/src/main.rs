//! duskcode: a full-screen terminal client for dusk_core. All logic lives in the core; this
//! crate only shows threads and passes on what the user types.

//!
//! - `cli`: arguments and the commands that run without the screen;
//! - `host`: the terminal, the clipboard and git, as the machine offers them;
//! - `app`: what the screen shows and how keys change it;
//! - `ui`: drawing it; `session` ties the app to the core.

mod app;
mod cli;
mod host;
mod session;
mod ui;

use anyhow::Result;
use cli::args::{self, Args};
use dusk_core::{init::logs, init::paths::Paths, release::update, runtime::Runtime};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    let paths = Paths::from_home()?;
    let (state_db, thread) = match args::parse_args() {
        Args::Run { state_db, thread } => (state_db, thread),
        Args::Remove => return cli::remove::run(&paths),
        Args::InstallResources { directory } => {
            dusk_core::release::resources::install_snapshot(&directory, update::VERSION)?;
            return Ok(());
        }
        Args::Update => {
            println!("{}", update::update_now(&paths).await?);
            return Ok(());
        }
    };
    if let Err(error) = logs::init(&paths, env!("CARGO_CRATE_NAME"), "tui") {
        eprintln!("duskcode: no log files: {error:#}");
    }
    log_panics();
    info!("duskcode {} starts", env!("CARGO_PKG_VERSION"));
    // Started before the screen is taken, so a first-start message stays readable.
    let runtime = Runtime::start(paths, state_db).await?;
    let terminal = host::terminal::enter()?;
    let result = session::run(&runtime, terminal, thread).await;
    host::terminal::leave();
    runtime.shutdown().await;
    match &result {
        Ok(()) => info!("duskcode exits"),
        Err(error) => error!("duskcode exits: {error:#}"),
    }
    result
}

/// A panic is written to the log of the side it happened in, since the screen it would
/// be printed on is about to be cleared.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let in_client = panic
            .location()
            .is_some_and(|location| location.file().contains("tui/src"));
        if in_client {
            error!("{panic}");
        } else {
            error!(target: "dusk_core::panic", "{panic}");
        }
        default(panic);
    }));
}
