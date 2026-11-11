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
mod screensaver;
mod session;
mod ui;

use anyhow::Result;
use cli::args::{self, Args};
use dusk_core::app::Paths;
use dusk_protocol::client::{Hello, PROTOCOL_VERSION};
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "macos")]
    if dusk_core::app::run_overlay_if_asked() {
        return Ok(());
    }
    // The built-in telegram plugin's platform: this binary, run by the core.
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("--platform") {
        return match arguments.next().as_deref() {
            Some("telegram") => dusk_telegram::platform::run().await,
            other => anyhow::bail!("no built-in platform {}", other.unwrap_or("named")),
        };
    }
    let (paths, state_db, thread, web) = match args::parse_args() {
        Args::Run {
            state_db,
            thread,
            web,
        } => (Paths::from_home()?, state_db, thread, web),
        Args::Headless(options) => return cli::run::run(options).await,
        Args::Restore { archive } => return cli::restore::run(&archive),
        Args::RecoverRestore => return cli::restore::recover(),
        Args::Remove(options) => return cli::remove::run(&Paths::from_home()?, &options),
        Args::InstallResources { directory } => {
            // The installer's archive is checked against the tag first; offline it stands.
            match dusk_core::app::complete(&directory, dusk_core::app::VERSION).await {
                Ok(0) => {}
                Ok(fetched) => eprintln!("fetched {fetched} resource files from GitHub"),
                Err(error) => eprintln!("resources not checked against GitHub: {error:#}"),
            }
            dusk_core::app::install_snapshot(&directory, dusk_core::app::VERSION)?;
            return Ok(());
        }
        Args::Update => {
            let paths = Paths::from_home()?;
            println!("{}", dusk_core::app::update_now(&paths).await?);
            return Ok(());
        }
    };
    let state_db = state_db
        .map(|path| dusk_core::app::resolve_state_db(&path))
        .transpose()?;
    // Keep a shared lock for the full process lifetime, including log initialization.
    let _restore_guard = dusk_core::app::RestoreGuard::startup(&paths, state_db.as_deref())?;
    let telemetry = match dusk_core::app::init_logs(&paths, env!("CARGO_CRATE_NAME"), "tui") {
        Ok(telemetry) => Some(telemetry),
        Err(error) => {
            eprintln!("duskcode: no log files: {error:#}");
            None
        }
    };
    log_panics();
    info!("duskcode {} starts", env!("CARGO_PKG_VERSION"));
    cli::resources::ensure().await;
    // Started before the screen is taken, so a first-start message stays readable.
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: session::THREAD_SOURCE.to_owned(),
    };
    let hub = dusk_core::app::start_hub(paths, state_db).await?;
    let mut client = hub.attach(hello)?;
    let web = match web {
        Some(options) => {
            let server = cli::web::start(Arc::clone(&hub), options).await?;
            cli::web::wait_for_enter().await;
            Some(server)
        }
        None => None,
    };
    let terminal = host::terminal::enter()?;
    let result = session::run(&mut client, terminal, thread).await;
    host::terminal::leave();
    // Browsers go first, so none of them calls a core that has stopped.
    if let Some(server) = web {
        server.stop().await;
    }
    client.shutdown().await;
    match &result {
        Ok(()) => info!("duskcode exits"),
        Err(error) => error!("duskcode exits: {error:#}"),
    }
    if let Some(telemetry) = telemetry {
        telemetry.shutdown();
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
