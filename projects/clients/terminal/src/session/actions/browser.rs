//! Actions that open a page in the system's browser: links and sign-ins.

use dusk_core::client::Client;

use crate::app::{App, Tone};

pub async fn sign_in_chatgpt(client: &Client, app: &mut App) {
    match client.settings().sign_in_chatgpt().await {
        Ok(page) => {
            let notice = match open_in_browser(&page) {
                // The address too, for a browser other than the system's own.
                true => format!("Sign-in with ChatGPT opened in the browser; or open {page}"),
                false => format!("Sign in with ChatGPT: open {page} in a browser"),
            };
            app.transcript.notice(notice, Tone::Info);
            app.flash = Some("the provider is saved once you have signed in".into());
        }
        Err(error) => app.flash = Some(format!("cannot sign in with ChatGPT: {error:#}")),
    }
}

pub async fn sign_in_mcp(client: &Client, app: &mut App, name: String) {
    match client.catalog().sign_in_mcp(&name).await {
        Ok(page) => {
            // The full address carries the sign-in's state; shown only when needed.
            let notice = match open_in_browser(&page) {
                true => format!(
                    "Sign-in to {name} opened in the browser at {}",
                    origin(&page)
                ),
                false => format!("Sign-in to {name}: open {page} in a browser"),
            };
            app.transcript.notice(notice, Tone::Info);
            app.flash = Some(format!("{name} starts again once you have signed in"));
        }
        Err(error) => app.flash = Some(format!("cannot sign in to {name}: {error:#}")),
    }
}

pub fn open_link(app: &mut App, url: String) {
    app.flash = Some(match open_in_browser(&url) {
        true => format!("opened {url}"),
        false => format!("open {url} in a browser"),
    });
}

/// The scheme and host of a page, without the path and query it carries.
fn origin(page: &str) -> String {
    page.splitn(4, '/').take(3).collect::<Vec<_>>().join("/")
}

/// The system's browser shows the page; false when none could be started.
fn open_in_browser(url: &str) -> bool {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}
