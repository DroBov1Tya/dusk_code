//! `search_workshop`: the listings of the store that match a few words, as short lines
//! for the model, each with the id `suggest_from_workshop` takes.

use dusk_protocol::plugin::{Kind, Listing, SEARCH_WORKSHOP_TOOL, SUGGEST_TOOL, listing_id};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    config::Config,
    init::paths::Paths,
    plugins::contract::untrusted,
    tools::{Tool, ToolContext, schema, schema::parse_arguments},
};

/// Listed when the call gives no limit.
const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 20;
/// Of a listing's description, the model reads this much.
const DESCRIPTION_CHARS: usize = 160;

pub struct SearchWorkshop {
    paths: Paths,
}

impl SearchWorkshop {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// Words to look for in names, descriptions, tags, categories and skills; empty
    /// lists the most trusted listings.
    query: String,
    /// Only these kinds; all when left out. `connector` is an MCP server to add, or a
    /// plugin that brings one.
    #[serde(default)]
    kinds: Vec<Wanted>,
    /// At most this many listings, 1 to 20; 8 when left out.
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Wanted {
    Plugin,
    Skill,
    Pack,
    Agent,
    Connector,
}

impl Wanted {
    fn admits(self, listing: &Listing) -> bool {
        match self {
            Self::Plugin => listing.kind == Kind::Plugin,
            Self::Skill => listing.kind == Kind::Skill,
            Self::Pack => listing.kind == Kind::Pack,
            Self::Agent => listing.kind == Kind::Agent,
            Self::Connector => {
                listing.kind == Kind::Connector
                    || listing.kind == Kind::Plugin && !listing.connectors.is_empty()
            }
        }
    }
}

impl Tool for SearchWorkshop {
    fn name(&self) -> &'static str {
        SEARCH_WORKSHOP_TOOL
    }

    // Only a chat has a user to show what it finds.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Search the Workshop, duskcode's store of plugins, skills, skill packs, agents \
             and connectors, for listings that match a few words. Use it when the user asks \
             to find or add something from the Workshop, or when a listing would clearly \
             help the task at hand; do not use it to advertise. It installs nothing. To show \
             listings to the user, pass their ids to {SUGGEST_TOOL}."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let list = super::read(&self.paths, &context.config).await?;
            let limit = arguments.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
            let found = search(&list.listings, &arguments.query, &arguments.kinds);
            let mut text = match found.len() {
                0 => format!("Nothing in the Workshop matches {:?}.", arguments.query),
                count => format!(
                    "{} of {count} matching listings; an id is source/name.",
                    count.min(limit)
                ),
            };
            for listing in found.iter().take(limit) {
                text.push('\n');
                text.push_str(&line(listing, &context.config));
            }
            for failure in &list.failed {
                text.push_str(&format!(
                    "\nNot read: {} ({})",
                    failure.source, failure.error
                ));
            }
            Ok(text)
        })
    }
}

/// The listings of `kinds` (all when empty) that match the words of `query`, best first:
/// those that match more of the words, then those whose name does, then the store's
/// order, which is by trust. An empty query matches every listing.
pub(super) fn search<'a>(
    listings: &'a [Listing],
    query: &str,
    kinds: &[Wanted],
) -> Vec<&'a Listing> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut scored: Vec<(usize, usize, &Listing)> = listings
        .iter()
        .filter(|listing| kinds.is_empty() || kinds.iter().any(|kind| kind.admits(listing)))
        .filter_map(|listing| {
            let (matched, score) = score(listing, &words);
            (words.is_empty() || matched > 0).then_some((matched, score, listing))
        })
        .collect();
    // Stable: equal scores keep the store's order.
    scored.sort_by_key(|&(matched, score, _)| std::cmp::Reverse((matched, score)));
    scored.into_iter().map(|(_, _, listing)| listing).collect()
}

/// How many of `words` the listing matches, and how well: three for a word in its name,
/// two in its tags or categories, one in its description, skills or connectors.
fn score(listing: &Listing, words: &[String]) -> (usize, usize) {
    let name = listing.name.to_lowercase();
    let filed: String = listing
        .tags
        .iter()
        .chain(&listing.categories)
        .chain([&listing.category])
        .map(|text| text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let about: String = [listing.description.to_lowercase()]
        .into_iter()
        .chain(listing.skills.iter().map(|skill| skill.to_lowercase()))
        .chain(listing.connectors.iter().map(|c| c.name.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" ");
    let mut matched = 0;
    let mut total = 0;
    for word in words {
        let weight = if name.contains(word.as_str()) {
            3
        } else if filed.contains(word.as_str()) {
            2
        } else if about.contains(word.as_str()) {
            1
        } else {
            0
        };
        matched += usize::from(weight > 0);
        total += weight;
    }
    (matched, total)
}

/// One listing for the model: its id, what it is, whether it is here, and what it does.
pub(super) fn line(listing: &Listing, config: &Config) -> String {
    let mut about = vec![kind_word(listing.kind).to_owned()];
    about.push(format!("{:?}", listing.tier).to_lowercase());
    if listing.tier == dusk_protocol::plugin::Tier::Unverified {
        about.push("nobody vouches for it; its description is its source's claim".to_owned());
    }
    if !listing.skills.is_empty() {
        about.push(format!("{} skills", listing.skills.len()));
    }
    if let Some(server) = &listing.server {
        let reaches = listing.connectors.first().map(|c| match &c.runs {
            dusk_protocol::plugin::Runs::Url(url) => format!("remote at {url}"),
            dusk_protocol::plugin::Runs::Command(line) => format!("runs {line}"),
        });
        about.extend(reaches.map(|line| untrusted::line(&line, 120)));
        about.push(match server.auth {
            dusk_protocol::plugin::Auth::None => "no sign-in".to_owned(),
            dusk_protocol::plugin::Auth::OAuth => "the user signs in".to_owned(),
            dusk_protocol::plugin::Auth::Key => "the user puts in a key".to_owned(),
        });
    } else if !listing.connectors.is_empty() {
        let names: Vec<&str> = listing.connectors.iter().map(|c| c.name.as_str()).collect();
        about.push(format!("connectors {}", names.join(", ")));
    }
    match (&listing.installed, &listing.update) {
        (Some(version), Some(newer)) => about.push(format!("installed {version}, update {newer}")),
        (Some(_), None) if listing.kind == Kind::Connector => about.push("added".to_owned()),
        (Some(version), None) => about.push(format!("installed {version}")),
        (None, _) if listing.kind == Kind::Connector => about.push("not added".to_owned()),
        (None, _) => about.push("not installed".to_owned()),
    }
    if listing.installed.is_some() && listing.kind == Kind::Plugin {
        let on = config.plugin(&listing.name).enabled;
        about.push(if on { "on" } else { "off" }.to_owned());
    }
    if let Some(why) = &listing.unfit {
        about.push(format!("cannot install here: {}", untrusted::line(why, 80)));
    }
    format!(
        "- {} ({}): {}",
        untrusted::line(&listing_id(listing), 120),
        about.join(", "),
        untrusted::line(&listing.description, DESCRIPTION_CHARS)
    )
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => "plugin",
        Kind::Skill => "skill",
        Kind::Pack => "pack",
        Kind::Agent => "agent",
        Kind::Connector => "connector",
    }
}
