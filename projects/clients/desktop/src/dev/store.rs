//! A made-up plugin store for the screenshot tool, so the Workshop can be photographed
//! before the real store is online: thirty listings of every tier and kind, some
//! installed, one with an update, one that cannot run here, one that asks for nothing,
//! one for the terminal only, two packs, a clash, covers, and a source that failed.

use std::sync::Arc;

use dusk_protocol::{
    catalog::{PackSkill, SkillPack},
    plugin::{Client, Cover, Kind, Listing, Os, SourceFailure, Tier, UpdateReport},
};
use gpui_kit::*;

use crate::{
    pages::connectors::{
        Shelf,
        plugins::{Confirm, workshop::covers::Cover as Drawn},
    },
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
        ..Listing::default()
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
            update: Some("2.1.0".into()),
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
            tier: [Tier::Official, Tier::Verified, Tier::Community][index % 3],
            source: ["official", "dusk-mirror", "friends"][index % 3].into(),
            clash: index == 4,
            ..listing(&name, description)
        });
    }
    let categories = [
        "automation",
        "desktop",
        "general",
        "devops",
        "writing",
        "data",
    ];
    for (index, listing) in all.iter_mut().enumerate() {
        if listing.source.is_empty() {
            listing.source = "official".into();
        }
        listing.category = categories[index % categories.len()].into();
        listing.stars = ((index * 37) % 180) as u32;
        if index % 2 == 0 {
            listing.cover = Some(Cover {
                path: format!("covers/{}.png", listing.name),
                sha256: format!("fake-{}", listing.name),
                size: 1,
            });
        }
    }
    all.insert(
        2,
        Listing {
            kind: Kind::Pack,
            permissions: Vec::new(),
            skills: vec![
                "duskcode-engineering/spike".into(),
                "duskcode-engineering/review".into(),
                "duskcode-engineering/refactor".into(),
            ],
            installed: Some("1.2.0".into()),
            version: "1.2.0".into(),
            category: "general".into(),
            stars: 128,
            source: "official".into(),
            ..listing(
                "duskcode-engineering",
                "Skills for engineering work: spikes, reviews and refactors.",
            )
        },
    );
    all.insert(
        4,
        Listing {
            kind: Kind::Agent,
            permissions: Vec::new(),
            category: "automation".into(),
            stars: 64,
            source: "official".into(),
            ..listing(
                "release-captain",
                "An agent that walks a release through its checklist.",
            )
        },
    );
    all.push(Listing {
        kind: Kind::Pack,
        tier: Tier::Community,
        source: "friends".into(),
        permissions: Vec::new(),
        skills: vec!["writing/edit".into(), "writing/outline".into()],
        category: "writing".into(),
        ..listing("writing", "Outlines and edits for prose.")
    });
    all.push(Listing {
        tier: Tier::Local,
        source: "local".into(),
        installed: Some("0.1.0".into()),
        version: "0.1.0".into(),
        permissions: Vec::new(),
        ..listing(
            "my-hooks",
            "Hooks of my own, dropped into the plugins folder.",
        )
    });
    all
}

fn packs() -> Vec<SkillPack> {
    let skills = |names: &[&str], off: usize| {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| PackSkill {
                name: (*name).into(),
                enabled: index != off,
            })
            .collect()
    };
    vec![
        SkillPack {
            name: "duskcode".into(),
            title: String::new(),
            description: String::new(),
            builtin: true,
            enabled: true,
            skills: skills(
                &[
                    "duskcode",
                    "skill-creator",
                    "skill-improve",
                    "skill-from-practice",
                    "computer-use",
                    "token-thrift",
                ],
                usize::MAX,
            ),
        },
        SkillPack {
            name: "duskcode-engineering".into(),
            title: "Engineering".into(),
            description: "Skills for engineering work.".into(),
            builtin: false,
            enabled: true,
            skills: skills(
                &[
                    "duskcode-engineering/spike",
                    "duskcode-engineering/review",
                    "duskcode-engineering/refactor",
                ],
                1,
            ),
        },
        SkillPack {
            name: "team-notes".into(),
            title: String::new(),
            description: String::new(),
            builtin: false,
            enabled: false,
            skills: skills(&["team-notes/standup", "team-notes/retro"], usize::MAX),
        },
    ]
}

/// A soft gradient per listing, so the covers can be told apart.
fn picture(seed: usize) -> Arc<Image> {
    use image::{ImageFormat, RgbImage};
    let hue = (seed * 47 % 360) as f32;
    let (w, h) = (300u32, 180u32);
    let picture = RgbImage::from_fn(w, h, |x, y| {
        let t = (x + y) as f32 / (w + h) as f32;
        let r = (120. + 90. * (hue.to_radians()).cos() * (1. - t)) as u8;
        let g = (110. + 80. * ((hue + 120.).to_radians()).cos() * t) as u8;
        let b = (150. + 90. * ((hue + 240.).to_radians()).cos()) as u8;
        image::Rgb([r, g, b])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    let _ = picture.write_to(&mut bytes, ImageFormat::Png);
    Arc::new(Image::from_bytes(
        gpui_kit::ImageFormat::Png,
        bytes.into_inner(),
    ))
}

/// `fake=store`: the made-up listings in the Workshop instead of the store's.
pub fn fake(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    let listings = listings();
    for (index, listing) in listings.iter().enumerate() {
        if let Some(cover) = &listing.cover {
            ws.workshop
                .covers
                .0
                .insert(cover.sha256.clone(), Drawn::Ready(picture(index)));
        }
    }
    let mut categories: Vec<String> = listings.iter().map(|l| l.category.clone()).collect();
    categories.sort();
    categories.dedup();
    ws.workshop.categories = categories;
    ws.workshop.listings = Some(listings);
    ws.workshop.packs = Some(packs());
    ws.workshop.failed_sources = vec![SourceFailure {
        source: "old-mirror".into(),
        error: "its list is not signed by the key it was added with".into(),
    }];
    ws.workshop.error = None;
    cx.notify();
}

/// `workshop`: the Workshop shown; `workshop=install:NAME|remove:NAME`: its window that
/// asks first; `workshop=busy:NAME`: the card at work; `workshop=failed:NAME`: the card
/// after a failure; `workshop=pack:NAME`: a pack's page; `workshop=report`: what
/// "Update all" did.
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
    if value == "report" {
        ws.workshop.report = Some(UpdateReport {
            updated: vec!["notes 2.1.0".into()],
            skipped: vec![(
                "telegram".into(),
                "it asks to reach https://api.example.org".into(),
            )],
        });
        cx.notify();
        return;
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
        ("pack", _) => ws.workshop.pack = Some(name.into()),
        ("install", Some(listing)) => ws.confirm_store(Confirm::Install(Box::new(listing)), cx),
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
        _ => eprintln!("workshop=install|remove|busy|failed|pack:NAME or report: {value}"),
    }
    cx.notify();
}
