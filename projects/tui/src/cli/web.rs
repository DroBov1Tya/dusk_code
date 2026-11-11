//! `--web`: the web client served beside the terminal one, over the same core.

use super::args::WebOptions;
use anyhow::Result;
use dusk_core::client::Hub;
use dusk_web::{Config, WebServer};
use std::sync::Arc;

/// Starts the server and tells the user, before the screen is taken, how to open it.
pub async fn start(hub: Arc<Hub>, options: WebOptions) -> Result<WebServer> {
    let exposed = !options.bind.is_loopback();
    let config = Config {
        bind: (options.bind, options.port).into(),
        allowed_hosts: options.allow_hosts,
    };
    let server = dusk_web::start(hub, config).await?;
    eprintln!("duskcode web: open this address once in your browser:");
    eprintln!("  {}", server.sign_in_url());
    eprintln!("The part after # is the sign-in token of this run; it never leaves your browser.");
    if exposed {
        eprintln!();
        eprintln!(
            "WARNING: the web client listens on {}, not only on this machine.",
            server.addr()
        );
        eprintln!("Anyone who can reach it and holds the token can use your agents and files.");
        eprintln!("The traffic is not encrypted; put it behind TLS or a tunnel you trust.");
    }
    Ok(server)
}

/// Gives the user time to copy the address before the terminal screen covers it.
pub async fn wait_for_enter() {
    eprintln!();
    eprintln!("Press Enter to open the terminal client.");
    let _ = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    })
    .await;
}
