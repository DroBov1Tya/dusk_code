//! duskcode: a full-screen terminal client for dusk_core. All logic lives in the core; this
//! crate only shows threads and passes on what the user types.

mod app;
mod args;
mod clipboard;
mod remove;
mod session;
mod ui;

use anyhow::Result;
use args::Args;
use dusk_core::{paths::Paths, runtime::Runtime};

#[tokio::main]
async fn main() -> Result<()> {
    let paths = Paths::from_home()?;
    let Args::Run { state_db, thread } = args::parse_args() else {
        return remove::run(&paths);
    };
    // Started before the screen is taken, so a first-start message stays readable.
    let runtime = Runtime::start(paths, state_db).await?;
    let terminal = ui::screen::enter()?;
    let result = session::run(&runtime, terminal, thread).await;
    ui::screen::leave();
    runtime.shutdown().await;
    result
}
