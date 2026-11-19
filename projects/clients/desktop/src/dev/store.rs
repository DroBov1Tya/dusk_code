//! A made-up plugin store for the screenshot tool, so the Workshop can be photographed
//! before the real store is online: twenty listings of every tier and kind, some
//! installed, one with an update, one that cannot run here, one that asks for nothing,
//! one for the terminal only, packs (two of the official list written by somebody else,
//! so Verified), a clash, two from a mirror with their repository's GitHub stars, and a
//! source that failed, drawn with the Workshop's real covers and screenshots. Nothing
//! here is named after a company or wears its logo. They are built in only with the
//! feature.

use std::sync::Arc;

use dusk_protocol::{
    catalog::{PackSkill, SkillPack},
    plugin::{
        Client, Connector, ConnectorState, Contents, Cover, FilePreview, FileRef, Kind, Listing,
        Os, PICKS, Picks, RepoStars, Runs, ScreenshotRef, SourceFailure, Tier, UpdateReport,
    },
};
use gpui_kit::*;

use crate::{
    modal::Modal,
    pages::connectors::{
        Shelf,
        plugins::{
            Confirm, Scope, View,
            workshop::{
                Shelf as StoreShelf,
                contents::{Loaded, Opened},
                covers::{self, Cover as Drawn},
                detail::Tab,
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
        author: "duskcode".into(),
        license: "MIT".into(),
        origin: Some(format!(
            "https://github.com/duskcode/workshop/tree/main/{name}"
        )),
        ..Listing::default()
    };
    let filed = |categories: &[&str]| -> Vec<String> {
        categories.iter().map(|item| (*item).to_owned()).collect()
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
            category: "coding".into(),
            categories: filed(&["coding", "review", "testing"]),
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
            categories: filed(&["security"]),
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
            // The core's lines: its program, its servers, then what else it may reach.
            permissions: vec![
                "run the program ./bin/telegram on this computer while it is on".into(),
                "start the MCP server telegram-history (./bin/telegram mcp) on this computer; \
                 the agent may use its tools"
                    .into(),
                "send the agent's requests for the tools of chat-search to \
                 https://search.example.com/mcp"
                    .into(),
                "Reach https://api.telegram.org".into(),
                "Use the secret TELEGRAM_TOKEN".into(),
            ],
            programs: vec!["./bin/telegram".into()],
            connectors: vec![
                Connector {
                    name: "telegram-history".into(),
                    runs: Runs::Command("./bin/telegram mcp".into()),
                    state: ConnectorState::Added,
                },
                Connector {
                    name: "chat-search".into(),
                    runs: Runs::Url("https://search.example.com/mcp".into()),
                    state: ConnectorState::Yours,
                },
            ],
            installed: Some("1.0.0".into()),
            category: "communication".into(),
            categories: filed(&["communication", "devices"]),
            ..listing(
                "telegram",
                "Chat with your agents from Telegram, with voice notes.",
            )
        },
        Listing {
            kind: Kind::Agent,
            permissions: Vec::new(),
            category: "coding".into(),
            categories: filed(&["coding"]),
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
            category: "devices".into(),
            categories: filed(&["devices", "testing"]),
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
            category: "devices".into(),
            categories: filed(&["devices"]),
            author: "phone-tools".into(),
            license: "Apache-2.0".into(),
            ..listing("vphone", "A virtual phone for testing calls and messages.")
        },
        // The official list's packs of somebody else's skills: Verified, not Official.
        Listing {
            kind: Kind::Pack,
            tier: Tier::Verified,
            permissions: Vec::new(),
            skills: vec![
                "skills-for-claude/documents".into(),
                "skills-for-claude/spreadsheets".into(),
                "skills-for-claude/slides".into(),
            ],
            category: "writing".into(),
            categories: filed(&["writing", "productivity"]),
            author: "skill-authors".into(),
            license: "Apache-2.0".into(),
            origin: Some("https://github.com/skill-authors/skills".into()),
            ..listing(
                "skills-for-claude",
                "Documents, spreadsheets and slides, written for Claude models.",
            )
        },
        Listing {
            kind: Kind::Pack,
            tier: Tier::Verified,
            permissions: Vec::new(),
            skills: vec!["skills-for-gpt/plan".into(), "skills-for-gpt/patch".into()],
            category: "coding".into(),
            categories: filed(&["coding"]),
            author: "skill-authors".into(),
            license: "MIT".into(),
            origin: Some("https://github.com/skill-authors/gpt-skills".into()),
            clash: true,
            ..listing(
                "skills-for-gpt",
                "Plans and patches, written for GPT models.",
            )
        },
        Listing {
            kind: Kind::Pack,
            tier: Tier::Community,
            source: "friends".into(),
            permissions: Vec::new(),
            skills: vec!["skills-for-gpt/plan".into()],
            category: "coding".into(),
            categories: filed(&["coding"]),
            // A mirror's claim, drawn as such.
            author: "duskcode".into(),
            clash: true,
            ..listing(
                "skills-for-gpt",
                "The same skills, through a friend's mirror.",
            )
        },
        Listing {
            kind: Kind::Pack,
            tier: Tier::Community,
            source: "friends".into(),
            permissions: Vec::new(),
            skills: vec![
                "gpu-computing-skills/kernels".into(),
                "gpu-computing-skills/profiling".into(),
            ],
            category: "data".into(),
            categories: filed(&["data", "coding"]),
            author: "friends-example".into(),
            license: "BSD-3-Clause".into(),
            origin: Some("https://github.com/friends-example/gpu-skills".into()),
            ..listing("gpu-computing-skills", "Writing and profiling GPU kernels.")
        },
        Listing {
            permissions: Vec::new(),
            homepage: None,
            tier: Tier::Community,
            source: "friends".into(),
            category: "writing".into(),
            categories: filed(&["writing"]),
            author: "friends-example".into(),
            ..listing(
                "word-count",
                "Counts the words of the shown file. Asks for nothing.",
            )
        },
        Listing {
            clients: vec![Client::Tui],
            tier: Tier::Community,
            source: "friends".into(),
            category: "productivity".into(),
            categories: filed(&["productivity", "devops"]),
            author: "friends-example".into(),
            homepage: Some("https://github.com/friends-example/tmux-panes".into()),
            repo_stars: Some(RepoStars {
                repo: "friends-example/tmux-panes".into(),
                stars: 122,
            }),
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
            categories: filed(&["writing"]),
            author: "friends-example".into(),
            homepage: Some("https://github.com/friends-example/writing".into()),
            repo_stars: Some(RepoStars {
                repo: "friends-example/writing".into(),
                stars: 37,
            }),
            ..listing("writing", "Outlines and edits for prose.")
        },
        Listing {
            tier: Tier::Local,
            source: "local".into(),
            installed: Some("0.1.0".into()),
            version: "0.1.0".into(),
            permissions: Vec::new(),
            author: String::new(),
            license: String::new(),
            origin: None,
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
            category: ["devops", "productivity"][index % 2].into(),
            categories: filed(&[["devops", "productivity"][index % 2]]),
            author: ["duskcode", "ops-tools", "friends-example"][index % 3].into(),
            ..listing(word, description)
        });
    }
    for (index, listing) in all.iter_mut().enumerate() {
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
        "duskcode-agents" => include_bytes!("media/duskcode-agents.webp"),
        "duskcode-engineering" => include_bytes!("media/duskcode-engineering.webp"),
        "duskcode-security" => include_bytes!("media/duskcode-security.webp"),
        "ios-simulator" => include_bytes!("media/ios-simulator.webp"),
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
                    "skill-evolve",
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

/// Single skills, as the Skills page's Workshop lists them: of each tier, one here.
pub fn skill_listings() -> Vec<Listing> {
    let skill = |name: &str, author: &str, tier: Tier, description: &str| Listing {
        name: name.into(),
        version: "1.0.0".into(),
        description: description.into(),
        kind: Kind::Skill,
        source: match tier {
            Tier::Community => "friends".into(),
            _ => "official".into(),
        },
        tier,
        author: author.into(),
        license: "MIT".into(),
        categories: vec!["coding".into()],
        created: Some("2026-11-02T00:00:00Z".into()),
        updated: Some("2026-11-12T00:00:00Z".into()),
        ..Listing::default()
    };
    vec![
        Listing {
            installed: Some("1.0.0".into()),
            ..skill(
                "commit-messages",
                "duskcode",
                Tier::Official,
                "Write commit subjects as area: imperative summary, with a body that says why.",
            )
        },
        skill(
            "changelog",
            "duskcode",
            Tier::Official,
            "Keep a changelog of fragments, one per change, merged at release.",
        ),
        skill(
            "sql-review",
            "Data Tools Collective",
            Tier::Verified,
            "Read a migration for locks, missing indexes and irreversible steps.",
        ),
        skill(
            "haiku-docs",
            "someone",
            Tier::Community,
            "Explain a module in three short lines before the details.",
        ),
    ]
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
/// listing's window, `workshop=gallery:N` its Nth picture, `workshop=tab:skills|contents|connectors`
/// that tab; `workshop=hero:N`: the Nth
/// featured pick; `workshop=next:N`: the Official shelf from its Nth capsule;
/// `workshop=all:TIER`: that tier's shelf whole; `workshop=tier:TIER`: filter by it;
/// `workshop=coming:NAME`: its cover still coming; `workshop=offline:`: the store could not
/// be read again, so covers not here are drawn offline; `workshop=filters:`: the filter menu open; `workshop=scroll:PX`: the page that far down; `workshop=report`: what "Update all" did.
pub fn step(ws: &mut Workspace, value: &str, window: &mut Window, cx: &mut Context<Workspace>) {
    ws.center = Center::Connectors;
    // `skills:` shows the Skills page's Workshop of single skills instead.
    let skills = value == "skills";
    ws.shelf = if skills {
        Shelf::Skills
    } else {
        Shelf::Plugins
    };
    ws.workshop.plugins_view = Some(View::Discover);
    ws.workshop.skills_view = Some(View::Discover);
    ws.workshop.scope = if skills {
        Scope::Skills
    } else {
        Scope::Plugins
    };
    if skills {
        let listings = skill_listings();
        ws.workshop.picks = picks(&listings);
        ws.workshop.listings = Some(listings);
    }
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
        ("", _) | ("skills", _) => {}
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
        ("tab", _) => {
            ws.workshop.tab = match name {
                "skills" => Tab::Skills,
                "contents" => Tab::Contents,
                "connectors" => Tab::Connectors,
                _ => Tab::Overview,
            };
            if let Some(Modal::Listing { source, name }) = &ws.modal {
                ws.workshop.contents = Some(contents(source, name));
            }
        }
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
        ("install", Some(listing)) => {
            ws.confirm_store(Confirm::Install(Box::new(listing)), window, cx)
        }
        ("remove", Some(_)) => ws.confirm_store(Confirm::Remove(name.into()), window, cx),
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
             tab:skills, tier|all:TIER, filters:, offline: or report: {value}"
        ),
    }
    cx.notify();
}

/// The files of a pack, the way the core lists them, with its first skill open.
fn contents(source: &str, name: &str) -> Opened {
    let file = |path: &str, size: u64, text: bool| FileRef {
        path: path.into(),
        size,
        text,
    };
    Opened {
        source: source.into(),
        name: name.into(),
        files: Loaded::Ready(Contents {
            files: vec![
                file("LICENSE", 1071, true),
                file("bin/helper", 2_411_520, false),
                file("pack.yml", 214, true),
                file("review/SKILL.md", 3_890, true),
                file("review/references/checklist.md", 1_204, true),
                file("spike/SKILL.md", 2_317, true),
                file("spike/scripts/time.sh", 402, true),
            ],
            package_sha256: "4f1c9a07be2d5e3f6a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f"
                .into(),
            installed: false,
        }),
        chosen: Some("spike/SKILL.md".into()),
        preview: Loaded::Ready(FilePreview {
            text: "---\ndescription: Answer one question with the smallest experiment.\n---\n\n\
                   # Spike\n\nWrite down the question first. Then build the least code that answers \
                   it, and **throw it away** once you know.\n\n- Time-box it: an hour, not a day.\n\
                   - Keep notes of what you tried.\n- Report the answer, not the code.\n"
                .into(),
            truncated: false,
        }),
    }
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
