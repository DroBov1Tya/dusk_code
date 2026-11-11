//! Serves the web client over a core of its own, for working on the client:
//! `HOME=/tmp/dusk-home cargo run -p dusk_web --example serve [port]`.

use dusk_core::app::{Paths, start_hub};
use dusk_web::{Config, start};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let hub = start_hub(Paths::from_home()?, None).await?;
    let config = Config {
        bind: ([127, 0, 0, 1], port).into(),
        ..Config::default()
    };
    let server = start(hub, config).await?;
    println!("open {}", server.sign_in_url());
    tokio::signal::ctrl_c().await?;
    server.stop().await;
    Ok(())
}
