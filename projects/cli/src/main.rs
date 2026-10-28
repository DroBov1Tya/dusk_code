mod app;
mod args;
mod chat;
mod config;
mod constants;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    config::init_logger();
    let args = args::parse_args();
    let app = app::Application::build(args).await?;
    app.run().await?;
    Ok(())
}
