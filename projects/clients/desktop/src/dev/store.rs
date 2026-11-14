//! A made-up plugin store for the screenshot tool, so the Workshop can be photographed
//! before the real store is online: thirty listings, some installed, one with an update,
//! one that cannot run here, one that asks for nothing, one for the terminal only.

use dusk_protocol::plugin::{Client, Listing, Os};
use gpui_kit::*;

use crate::{
    pages::connectors::{Shelf, plugins::Confirm},
    workspace::{Center, Workspace},
};

pub fn listings() -> Vec<Listing> {
    let listing = |name: &str, description: &str| Listing {
        name: name.into(),
        version: "1.0.0".into(),
        description: description.into(),
        platforms: Vec::new(),
        clients: Vec::new(),
        permissions: vec!["Reach https://api.example.com".into()],
        homepage: Some(format!("https://example.com/{name}")),
        installed: None,
        unfit: None,
    };
    let mut all = vec![
        Listing {
            platforms: vec![Os::Macos],
            clients: vec![Client::Gui, Client::Tui],
            permissions: vec![
                "Reach https://api.telegram.org".into(),
                "Use the secret TELEGRAM_TOKEN".into(),
            ],
            installed: Some("1.0.0".into()),
            ..listing(
                "telegram",
                "Chat with your agents from Telegram, with voice notes.",
            )
        },
        Listing {
            version: "2.1.0".into(),
            installed: Some("2.0.3".into()),
            ..listing(
                "notes",
                "Scratch notes kept beside the work, one list per project.",
            )
        },
        Listing {
            platforms: vec![Os::Macos],
            clients: vec![Client::Gui],
            permissions: vec!["Run xcrun simctl".into()],
            unfit: Some("It runs on macOS only".into()),
            ..listing(
                "ios-simulator",
                "Drive the iOS Simulator and see its screen in a panel.",
            )
        },
        Listing {
            permissions: Vec::new(),
            homepage: None,
            ..listing(
                "word-count",
                "Counts the words of the shown file. Asks for nothing.",
            )
        },
        Listing {
            clients: vec![Client::Tui],
            ..listing(
                "tmux-panes",
                "Opens each agent in its own tmux pane of the terminal.",
            )
        },
    ];
    let words = [
        (
            "metrics",
            "Charts of the project's metrics from Prometheus.",
        ),
        (
            "logs",
            "Tails and searches the logs of the running services.",
        ),
        ("traces", "Shows the traces of a request across services."),
        (
            "queue",
            "Watches the job queue and retries the failed jobs.",
        ),
        ("deploy", "Deploys the shown branch to the staging cluster."),
        ("preview", "Opens a preview of the web app for each change."),
        (
            "tickets",
            "Links chats to the tickets of the issue tracker.",
        ),
        ("wiki", "Reads and writes pages of the team's wiki."),
        (
            "translator",
            "Translates a message before it is sent, and the answers back.",
        ),
        (
            "calendar",
            "Reads the calendar so agents plan around meetings.",
        ),
    ];
    for (index, (word, description)) in words.iter().cycle().take(25).enumerate() {
        let name = format!("{word}-{}", index / words.len() + 1);
        all.push(Listing {
            version: format!("0.{}.{}", index % 4 + 1, index % 3),
            platforms: if index % 3 == 0 {
                vec![Os::Macos, Os::Linux]
            } else {
                Vec::new()
            },
            installed: (index % 7 == 2).then(|| format!("0.{}.{}", index % 4 + 1, index % 3)),
            ..listing(&name, description)
        });
    }
    all
}

/// `fake=store`: the made-up listings in the Workshop instead of the store's.
pub fn fake(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    ws.workshop.listings = Some(listings());
    ws.workshop.error = None;
    cx.notify();
}

/// `workshop`: the Workshop shown; `workshop=install:NAME|remove:NAME`: its window that
/// asks first; `workshop=busy:NAME`: the row at work; `workshop=failed:NAME`: the row
/// after a failure.
pub fn step(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    ws.center = Center::Connectors;
    ws.shelf = Shelf::Plugins;
    ws.workshop.open = true;
    if ws.settings.is_none() {
        ws.load_settings(cx);
    }
    if ws.workshop.listings.is_none() {
        ws.load_store(cx);
    }
    let (what, name) = value.split_once(':').unwrap_or((value, ""));
    let listing = ws
        .workshop
        .listings
        .iter()
        .flatten()
        .find(|listing| listing.name == name)
        .cloned();
    match (what, listing) {
        ("", _) => {}
        ("install", Some(listing)) => ws.confirm_store(Confirm::Install(listing), cx),
        ("remove", Some(_)) => ws.confirm_store(Confirm::Remove(name.into()), cx),
        ("busy", Some(_)) => {
            ws.workshop.busy.insert(name.into(), "Installing...");
        }
        ("failed", Some(_)) => {
            ws.workshop.failed.insert(
                name.into(),
                "cannot install: the package size differs from the list".into(),
            );
        }
        _ => eprintln!("workshop=install|remove|busy|failed:NAME of a listing: {value}"),
    }
    cx.notify();
}
