//! duskcode-gui: the desktop client. It only draws and calls the core through the
//! protocol, the same way the TUI does:
//! - `core`: the bridge from the window to the core's client;
//! - `workspace`: the window's state and its three columns;
//! - `sidebar`: projects, chats, recent chats and search, on the left;
//! - `connectors`: MCP servers, skills, plugins, tools and the agents' graph;
//! - `chat`: the transcript, the composer and the bar under it, in the middle;
//! - `inspector`: git, files, context, todo, agents and jobs, on the right;
//! - `settings`: the settings tab;
//! - `theme`: the color themes; `host`: what the machine opens, such as a terminal.

mod chat;
mod connectors;
mod core;
mod host;
mod inspector;
mod settings;
mod sidebar;
mod theme;
mod widgets;
mod workspace;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use dusk_core::app::{self, Paths};
use dusk_protocol::client::{Hello, PROTOCOL_VERSION};
use gpui_kit::*;

/// The source the threads this client creates carry.
pub const THREAD_SOURCE: &str = "gui";

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let paths = Paths::from_home()?;
    let _restore_guard = app::RestoreGuard::startup(&paths, None)?;
    let telemetry = app::init_logs(&paths, env!("CARGO_CRATE_NAME"), "gui").ok();
    let theme_file = paths.config_root().join("gui-theme");
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: THREAD_SOURCE.to_owned(),
    };
    let client = runtime.block_on(app::start(paths, None, hello))?;
    let (client, events, status) = core::split(client);
    let shutdown = Arc::clone(&client);
    let runtime = Arc::new(runtime);
    let quit_runtime = Arc::clone(&runtime);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(theme_file, cx);
            let core = core::Core::new(client, Arc::clone(&runtime));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("duskcode".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| workspace::Workspace::new(core, events, status, window, cx))
            });
            if let Err(error) = opened {
                tracing::error!("cannot open the window: {error:#}");
                cx.quit();
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.on_app_quit(move |_| {
                // Threads end their turns and the core stops before the process does.
                quit_runtime.block_on(async {
                    let stop = shutdown.shutdown();
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), stop).await;
                });
                async {}
            })
            .detach();
        });
    if let Some(telemetry) = telemetry {
        telemetry.shutdown();
    }
    Ok(())
}
