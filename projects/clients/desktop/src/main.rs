//! duskcode-gui: the desktop client. It only draws and calls the core through the
//! protocol, the same way the TUI does. The tree follows what the window is made of:
//! - `core`: the bridge from the window to the core's client;
//! - `workspace`: the window's state and its three columns;
//! - `columns`: the side columns: the `sidebar` (projects, chats, search) and the
//!   `inspector` (git, files, context, todo, agents, jobs);
//! - `pages`: what the middle column shows: the `chat`, the `artifacts` the agents made, the
//!   `connectors` (servers, skills, plugins, tools, speech), the `settings`, the `review` of
//!   changes and a `file` in full;
//! - `modal`: what opens over the window: search, a new project, a file viewer, confirmations;
//! - `ui`: the design system: its scale and pieces, the `widgets` and the `theme`;
//! - `platform`: what the window takes from the machine: the icon and notifications;
//! - `keys`: the keyboard shortcuts.

mod columns;
mod core;
mod keys;
mod modal;
mod pages;
mod platform;
#[cfg(feature = "screenshot")]
mod screenshot;
mod ui;
mod workspace;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use dusk_core::app::{self, Paths};
use dusk_protocol::client::{Hello, PROTOCOL_VERSION};
use gpui_kit::*;

/// The source the threads this client creates carry.
pub const THREAD_SOURCE: &str = "gui";

fn main() -> Result<()> {
    // The agent's own cursor runs as this binary again; without this the helper would
    // open a second window instead of the cursor.
    #[cfg(target_os = "macos")]
    if dusk_core::app::run_overlay_if_asked() {
        return Ok(());
    }
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
    let hub = runtime.block_on(app::start_hub(paths, None))?;
    let client = hub.attach(hello)?;
    let (client, events, status) = core::split(client);
    let shutdown = Arc::clone(&client);
    let runtime = Arc::new(runtime);
    let quit_runtime = Arc::clone(&runtime);

    // Pictures in answers load over HTTPS through this client; see chat::messages.
    let http = reqwest_client::ReqwestClient::user_agent("duskcode-gui")?;
    gpui_kit::application()
        .with_assets(ui::icons::Assets)
        .with_http_client(Arc::new(http))
        .run(move |cx| {
            gpui_kit::init(cx);
            keys::bind(cx);
            platform::icon::set_dock_icon();
            ui::theme::init(look_file, cx);
            let core = core::Core::new(client, Arc::clone(&runtime), Arc::clone(&hub));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("duskcode".into()),
                    ..Default::default()
                }),
                app_id: Some(platform::icon::APP_ID.to_owned()),
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
