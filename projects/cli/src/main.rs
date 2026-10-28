mod app;
mod args;
mod chat;
mod commands;
mod config;
mod constants;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    config::init_logger();
    app::run(args::parse_args()).await
}
