//! dusk: a full-screen terminal client for dusk_core. All logic lives in the core; this
//! crate only shows threads and passes on what the user types.

mod app;
mod args;
mod session;
mod ui;

use anyhow::Result;
use dusk_core::{paths::Paths, runtime::Runtime};

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::parse_args();
    // Started before the screen is taken, so a first-start message stays readable.
    let runtime = Runtime::start(Paths::from_home()?, args.state_db).await?;
    let terminal = ui::screen::enter()?;
    let result = session::run(&runtime, terminal, args.thread).await;
    ui::screen::leave();
    runtime.shutdown().await;
    result
}
