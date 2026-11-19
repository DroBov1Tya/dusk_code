//! `suggest_from_workshop`: a card of listings in the chat, built from the store's own
//! listings by the ids the model gave. Its result holds the card, so the chat draws it
//! from the stored result after a reload as it did live.

use anyhow::{Result, bail, ensure};
use dusk_protocol::plugin::{
    Listing, MAX_LABEL_CHARS, MAX_SUGGESTED, SEARCH_WORKSHOP_TOOL, SUGGEST_TOOL, Suggested,
    Suggestion, listing_id,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    init::paths::Paths,
    plugins::contract::untrusted,
    tools::{Tool, ToolContext, schema, schema::parse_arguments},
};

pub struct SuggestFromWorkshop {
    paths: Paths,
}

impl SuggestFromWorkshop {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// Ids of listings as search_workshop gives them, source/name; 1 to 6.
    ids: Vec<String>,
    /// The card's heading: one short line, such as "Design and animation skills".
    label: String,
}

impl Tool for SuggestFromWorkshop {
    fn name(&self) -> &'static str {
        SUGGEST_TOOL
    }

    // Only a chat has a user to show a card to.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Show the user a card of Workshop listings they can add with one click, each \
             with its own confirmation. Use it when the user asks to find or add something \
             from the Workshop, or when a listing would clearly help the task at hand; not \
             to advertise, and at most one card a turn. Give 1 to {MAX_SUGGESTED} ids that \
             {SEARCH_WORKSHOP_TOOL} listed and a short heading. It installs and switches on \
             nothing: the user decides on the card."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            // The call's own checks first, so a wrong call costs no store read.
            check(&arguments.ids)?;
            let list = super::read(&self.paths, &context.config).await?;
            let card = card(&list.listings, &arguments.ids, &arguments.label)?;
            Ok(card.result())
        })
    }
}

fn check(ids: &[String]) -> Result<()> {
    ensure!(
        (1..=MAX_SUGGESTED).contains(&ids.len()),
        "give 1 to {MAX_SUGGESTED} ids, not {}",
        ids.len()
    );
    Ok(())
}

/// The card of `ids` from the store's `listings`, under `label` made one plain line. An
/// id is `source/name`; a bare name is taken when one source alone offers it. An id the
/// store does not have refuses the whole card, naming it.
pub(super) fn card(listings: &[Listing], ids: &[String], label: &str) -> Result<Suggestion> {
    check(ids)?;
    let label = untrusted::line(label.trim(), MAX_LABEL_CHARS);
    let label = label.trim().to_owned();
    ensure!(!label.is_empty(), "the label is empty");
    let mut rows: Vec<Suggested> = Vec::new();
    let mut unknown = Vec::new();
    for id in ids {
        let id = id.trim();
        let found = match listings.iter().find(|listing| listing_id(listing) == id) {
            Some(listing) => Some(listing),
            None => by_name(listings, id)?,
        };
        match found {
            Some(listing) => {
                let row = Suggested::of(listing);
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
            None => unknown.push(untrusted::line(id, 80)),
        }
    }
    if !unknown.is_empty() {
        bail!(
            "not in the Workshop: {}; use ids that {SEARCH_WORKSHOP_TOOL} listed",
            unknown.join(", ")
        );
    }
    Ok(Suggestion {
        label,
        listings: rows,
    })
}

/// The one listing named `name` without its source; an error when several sources offer
/// it, since a mirror may copy a name.
fn by_name<'a>(listings: &'a [Listing], name: &str) -> Result<Option<&'a Listing>> {
    if name.contains('/') {
        return Ok(None);
    }
    let named: Vec<&Listing> = listings
        .iter()
        .filter(|listing| listing.name == name)
        .collect();
    match named[..] {
        [] => Ok(None),
        [listing] => Ok(Some(listing)),
        _ => {
            let ids: Vec<String> = named.iter().map(|listing| listing_id(listing)).collect();
            bail!(
                "{name} is offered by several sources; give one of {}",
                ids.join(", ")
            )
        }
    }
}
