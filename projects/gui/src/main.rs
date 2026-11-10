//! duskcode-gui: the desktop client. It only draws and calls the core through the
//! protocol, the same way the TUI does:
//! - `core`: the bridge from the window to the core's client;
//! - `workspace`: the window's state and its three columns;
//! - `sidebar`: projects, chats, recent chats and search, on the left;
//! - `connectors`: MCP servers, skills, plugins, tools and the agents' graph;
//! - `chat`: the transcript, the composer and the bar under it, in the middle;
//! - `inspector`: git, files, context, todo, agents and jobs, on the right;
//! - `artifacts`: the files the agents made, by kind, in the middle column;
//! - `modal`: what opens over the window: search, a new project, a file viewer;
//! - `settings`: the settings page with its category menu;
//! - `theme`: the color themes and fonts.

mod artifacts;
mod chat;
mod connectors;
mod core;
mod file_view;
mod icon;
mod inspector;
mod modal;
mod notify;
mod review;
#[cfg(feature = "screenshot")]
mod screenshot;
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
    // The installer checks what it put in place with this.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("duskcode-gui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let paths = Paths::from_home()?;
    let _restore_guard = app::RestoreGuard::startup(&paths, None)?;
    let telemetry = app::init_logs(&paths, env!("CARGO_CRATE_NAME"), "gui").ok();
    let look_file = paths.config_root().join("gui.json");
    let home = paths.workspace_root().to_path_buf();
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: THREAD_SOURCE.to_owned(),
    };
    let client = runtime.block_on(app::start(paths, None, hello))?;
    let (client, events, status) = core::split(client);
    let shutdown = Arc::clone(&client);
    let runtime = Arc::new(runtime);
    let quit_runtime = Arc::clone(&runtime);

    // Pictures in answers load over HTTPS through this client; see chat::messages.
    let http = reqwest_client::ReqwestClient::user_agent("duskcode-gui")?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_http_client(Arc::new(http))
        .run(move |cx| {
            gpui_kit::init(cx);
            icon::set_dock_icon();
            theme::init(look_file, cx);
            let core = core::Core::new(client, Arc::clone(&runtime));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("duskcode".into()),
                    ..Default::default()
                }),
                app_id: Some(icon::APP_ID.to_owned()),
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| workspace::Workspace::new(core, home, events, status, window, cx))
            });
            match opened {
                #[cfg(feature = "screenshot")]
                Ok(handle) => screenshot::run(handle, cx),
                #[cfg(not(feature = "screenshot"))]
                Ok(_) => {}
                Err(error) => {
                    tracing::error!("cannot open the window: {error:#}");
                    cx.quit();
                }
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
