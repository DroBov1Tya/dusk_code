//! A made-up plugin store for the screenshot tool, so the Workshop can be photographed
//! before the real store is online: twenty listings of every tier and kind, some
//! installed, one with an update, one that cannot run here, one that asks for nothing,
//! one for the terminal only, packs, a clash, and a source that failed, drawn with the
//! Workshop's real covers and screenshots. They are built in only with the feature.

use std::sync::Arc;

use dusk_protocol::{
    catalog::{PackSkill, SkillPack},
    plugin::{
        Client, Cover, Kind, Listing, Os, PICKS, Picks, ScreenshotRef, SourceFailure, Tier,
        UpdateReport,
    },
};
use gpui_kit::*;

use crate::{
    modal::Modal,
    pages::connectors::{
        Shelf,
        plugins::{
            Confirm,
            workshop::{
                Shelf as StoreShelf,
                covers::{self, Cover as Drawn},
            },
        },
    },
    workspace::{Center, Workspace},
};

pub fn listings() -> Vec<Listing> {
    let listing = |name: &str, description: &str| Listing {
        name: name.into(),
        version: "1.0.0".into(),
        description: description.into(),
        permissions: vec!["Reach https://api.example.com".into()],
        homepage: Some(format!("https://example.com/{name}")),
        source: "official".into(),
        ..Listing::default()
    };
    let shots = |captions: &[&str]| -> Vec<ScreenshotRef> {
        captions
            .iter()
            .map(|caption| ScreenshotRef {
                caption: Some((*caption).into()),
            })
            .collect()
    };
    let mut all = vec![
        Listing {
            kind: Kind::Pack,
            permissions: Vec::new(),
            skills: vec![
                "duskcode-engineering/spike".into(),
                "duskcode-engineering/review".into(),
                "duskcode-engineering/refactor".into(),
                "duskcode-engineering/debugging".into(),
            ],
            installed: Some("1.2.0".into()),
            version: "1.2.0".into(),
            category: "engineering".into(),
            tags: vec!["review".into(), "refactor".into()],
            screenshots: shots(&[
                "The pack's page with its skills",
                "Skills switched one by one",
                "Debugging a failing test",
            ]),
            ..listing(
                "duskcode-engineering",
                "Skills for engineering work: spikes, reviews, refactors and debugging, \
                 written from practice and kept short so they cost few tokens.",
            )
        },
        Listing {
            kind: Kind::Pack,
            permissions: Vec::new(),
            skills: vec![
                "duskcode-security/threat-model".into(),
                "duskcode-security/web-audit".into(),
            ],
            version: "0.9.0".into(),
            category: "security".into(),
            tags: vec!["owasp".into()],
            screenshots: shots(&["Its skills", "A web audit under way"]),
            ..listing(
                "duskcode-security",
                "Threat models and web audits against OWASP ASVS, with findings mapped to CWE.",
            )
        },
        Listing {
            platforms: vec![Os::Macos],
            clients: vec![Client::Gui, Client::Tui],
            permissions: vec![
                "Reach https://api.telegram.org".into(),
                "Use the secret TELEGRAM_TOKEN".into(),
            ],
            installed: Some("1.0.0".into()),
            category: "chat".into(),
            ..listing(
                "telegram",
                "Chat with your agents from Telegram, with voice notes.",
            )
        },
        Listing {
            kind: Kind::Agent,
            permissions: Vec::new(),
            category: "automation".into(),
            ..listing(
                "duskcode-agents",
                "Agents that plan, build and review, each with its own model and tools.",
            )
        },
        Listing {
            platforms: vec![Os::Macos],
            clients: vec![Client::Gui],
            permissions: vec!["Run xcrun simctl".into()],
            unfit: Some("It runs on macOS only".into()),
            category: "desktop".into(),
            ..listing(
                "ios-simulator",
                "Drive the iOS Simulator and see its screen in a panel.",
            )
        },
        Listing {
            version: "2.1.0".into(),
            installed: Some("2.0.3".into()),
            update: Some("2.1.0".into()),
            tier: Tier::Verified,
            source: "dusk-mirror".into(),
            category: "desktop".into(),
            ..listing("vphone", "A virtual phone for testing calls and messages.")
        },
        Listing {
            tier: Tier::Verified,
            source: "dusk-mirror".into(),
            category: "providers".into(),
            ..listing(
                "anthropic",
                "Claude models, with prompt caching and extended thinking.",
            )
        },
        Listing {
            tier: Tier::Verified,
            source: "dusk-mirror".into(),
            category: "providers".into(),
            clash: true,
            ..listing("openai", "GPT models through the Responses API.")
        },
        Listing {
            tier: Tier::Community,
            source: "friends".into(),
            category: "providers".into(),
            clash: true,
            ..listing("openai", "GPT models through a proxy of a friend.")
        },
        Listing {
            tier: Tier::Community,
            source: "friends".into(),
            category: "providers".into(),
            ..listing("nvidia", "NVIDIA NIM models for local GPUs.")
        },
        Listing {
            permissions: Vec::new(),
            homepage: None,
            tier: Tier::Community,
            source: "friends".into(),
            category: "writing".into(),
            ..listing(
                "word-count",
                "Counts the words of the shown file. Asks for nothing.",
            )
        },
        Listing {
            clients: vec![Client::Tui],
            tier: Tier::Community,
            source: "friends".into(),
            category: "automation".into(),
            ..listing(
                "tmux-panes",
                "Opens each agent in its own tmux pane of the terminal.",
            )
        },
        Listing {
            kind: Kind::Pack,
            tier: Tier::Community,
            source: "friends".into(),
            permissions: Vec::new(),
            skills: vec!["writing/edit".into(), "writing/outline".into()],
            category: "writing".into(),
            ..listing("writing", "Outlines and edits for prose.")
        },
        Listing {
            tier: Tier::Local,
            source: "local".into(),
            installed: Some("0.1.0".into()),
            version: "0.1.0".into(),
            permissions: Vec::new(),
            ..listing(
                "my-hooks",
                "Hooks of my own, dropped into the plugins folder.",
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
        ("deploy", "Deploys the shown branch to the staging cluster."),
        (
            "tickets",
            "Links chats to the tickets of the issue tracker.",
        ),
        (
            "calendar",
            "Reads the calendar so agents plan around meetings.",
        ),
    ];
    for (index, (word, description)) in words.iter().enumerate() {
        all.push(Listing {
            version: format!("0.{}.{}", index % 4 + 1, index % 3),
            tier: [Tier::Official, Tier::Verified, Tier::Community][index % 3],
            source: ["official", "dusk-mirror", "friends"][index % 3].into(),
            category: ["devops", "automation"][index % 2].into(),
            ..listing(word, description)
        });
    }
    for (index, listing) in all.iter_mut().enumerate() {
        listing.stars = ((index * 37 + 11) % 180) as u32;
        listing.created = Some(format!(
            "2026-{:02}-{:02}T10:00:00Z",
            1 + index % 10,
            1 + index * 7 % 27
        ));
        listing.updated = Some(format!("2026-11-{:02}T10:00:00Z", 1 + index * 5 % 16));
        if art(&listing.name).is_some() {
            listing.cover = Some(Cover {
                path: format!("covers/{}.webp", listing.name),
                sha256: format!("fake-{}", listing.name),
                size: 1,
            });
        }
    }
    all
}

/// The Workshop's own art for the listings that have it.
fn art(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "anthropic" => include_bytes!("media/anthropic.webp"),
        "duskcode-agents" => include_bytes!("media/duskcode-agents.webp"),
        "duskcode-engineering" => include_bytes!("media/duskcode-engineering.webp"),
        "duskcode-security" => include_bytes!("media/duskcode-security.webp"),
        "ios-simulator" => include_bytes!("media/ios-simulator.webp"),
        "nvidia" => include_bytes!("media/nvidia.webp"),
        "openai" => include_bytes!("media/openai.webp"),
        "telegram" => include_bytes!("media/telegram.webp"),
        "vphone" => include_bytes!("media/vphone.webp"),
        _ => return None,
    })
}

/// The Workshop's demo screenshots of the two packs that have them.
fn screenshots(name: &str) -> Vec<&'static [u8]> {
    match name {
        "duskcode-engineering" => vec![
            include_bytes!("media/engineering-1.webp"),
            include_bytes!("media/engineering-2.webp"),
            include_bytes!("media/engineering-3.webp"),
        ],
        "duskcode-security" => vec![
            include_bytes!("media/security-1.webp"),
            include_bytes!("media/security-2.webp"),
        ],
        _ => Vec::new(),
    }
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

fn webp(bytes: &'static [u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(
        gpui_kit::ImageFormat::Webp,
        bytes.to_vec(),
    ))
}

/// The core's picks, made here the way the core makes them.
fn picks(listings: &[Listing]) -> Picks {
    let top = |key: &dyn Fn(&Listing) -> String| {
        let mut order: Vec<usize> = (0..listings.len()).collect();
        order.sort_by_key(|&index| std::cmp::Reverse(key(&listings[index])));
        order.truncate(PICKS);
        order
    };
    Picks {
        newest: top(&|listing| listing.created.clone().unwrap_or_default()),
        updated: top(&|listing| listing.updated.clone().unwrap_or_default()),
        stars: top(&|listing| format!("{:06}", listing.stars)),
    }
}

/// `fake=store`: the made-up listings in the Workshop instead of the store's, with the
/// Workshop's real covers and screenshots.
pub fn fake(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    let listings = listings();
    for listing in &listings {
        if let (Some(cover), Some(bytes)) = (&listing.cover, art(&listing.name)) {
            ws.workshop
                .covers
                .0
                .insert(cover.sha256.clone(), Drawn::Ready(webp(bytes)));
        }
        let shots: Vec<Drawn> = screenshots(&listing.name)
            .into_iter()
            .map(|bytes| Drawn::Ready(webp(bytes)))
            .collect();
        if !shots.is_empty() {
            ws.workshop.shots.0.insert(covers::key(listing), shots);
        }
    }
    let mut categories: Vec<String> = listings
        .iter()
        .map(|l| l.category.clone())
        .filter(|category| !category.is_empty())
        .collect();
    categories.sort();
    categories.dedup();
    ws.workshop.categories = categories;
    ws.workshop.picks = picks(&listings);
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
/// asks first; `workshop=busy:NAME`: the capsule at work; `workshop=failed:NAME`: the
/// capsule after a failure; `workshop=pack:NAME`: a pack's page; `workshop=open:NAME`: a
/// listing's window, `workshop=gallery:N` its Nth picture; `workshop=hero:N`: the Nth
/// featured pick; `workshop=next:N`: the Official shelf from its Nth capsule;
/// `workshop=all:TIER`: that tier's shelf whole; `workshop=tier:TIER`: filter by it;
/// `workshop=coming:NAME`: its cover still coming; `workshop=offline:`: the store could not
/// be read again, so covers not here are drawn offline; `workshop=filters:`: the filter menu open; `workshop=scroll:PX`: the page that far down; `workshop=report`: what "Update all" did.
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
        ("open", Some(listing)) => {
            ws.workshop.gallery = 0;
            ws.workshop.filters = false;
            ws.modal = Some(Modal::Listing {
                source: listing.source,
                name: listing.name,
            });
        }
        ("gallery", _) => ws.workshop.gallery = name.parse().unwrap_or(0),
        ("hero", _) => ws.workshop.hero = name.parse().unwrap_or(0),
        ("filters", _) => ws.workshop.filters = true,
        ("scroll", _) => ws
            .shelf_scroll
            .set_offset(point(px(0.), px(-name.parse::<f32>().unwrap_or(0.)))),
        ("tier", _) => {
            let tier = tier_named(name);
            ws.workshop.tiers.extend(tier);
        }
        ("next", _) => {
            ws.workshop
                .scrolled
                .insert(StoreShelf::Tier(Tier::Official), name.parse().unwrap_or(0));
        }
        ("all", _) => {
            ws.workshop.shelf = Some(StoreShelf::Tier(tier_named(name).unwrap_or_default()))
        }
        ("install", Some(listing)) => ws.confirm_store(Confirm::Install(Box::new(listing)), cx),
        ("remove", Some(_)) => ws.confirm_store(Confirm::Remove(name.into()), cx),
        ("busy", Some(_)) => {
            ws.workshop.busy.insert(name.into(), "Installing...");
        }
        // The cover still coming; with `offline:` as well, one not fetched while offline.
        ("coming", Some(listing)) => {
            if let Some(cover) = &listing.cover {
                ws.workshop
                    .covers
                    .0
                    .insert(cover.sha256.clone(), Drawn::Loading);
            }
        }
        ("offline", _) => {
            ws.workshop.error = Some("the network is unreachable".into());
        }
        ("failed", Some(_)) => {
            ws.workshop.failed.insert(
                name.into(),
                "cannot install: the package size differs from the list".into(),
            );
        }
        _ => eprintln!(
            "workshop=install|remove|busy|failed|coming|pack|open:NAME, gallery|hero|next:N, \
             tier|all:TIER, filters:, offline: or report: {value}"
        ),
    }
    cx.notify();
}

fn tier_named(name: &str) -> Option<Tier> {
    match name {
        "official" => Some(Tier::Official),
        "verified" => Some(Tier::Verified),
        "community" => Some(Tier::Community),
        "local" => Some(Tier::Local),
        _ => None,
    }
}
