//! Measures how well agents do: benchmark tasks with automatic checks, run through the
//! core without a client. See evals/README.md.

mod args;
mod run;
mod sandbox;
mod score;
mod task;

use anyhow::Result;

use args::Args;

#[tokio::main]
async fn main() -> Result<()> {
    match args::parse_args() {
        Args::Selfcheck { tasks, only } => sandbox::selfcheck(&task::load(&tasks, &only)?),
        Args::Compare { before, after } => score::compare(&before, &after),
        Args::Run(options) => run::all(&options).await,
    }
}
