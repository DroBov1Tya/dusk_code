//! `pack.yml`, what a pack of skills or agents says about itself. A field it does not
//! know refuses the pack, so a typo never installs half a pack.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::is_word;

pub const FILE: &str = "pack.yml";
const MAX_BYTES: usize = 64 * 1024;
const MAX_NAME: usize = 64;
const MAX_ITEMS: usize = 256;
const MAX_TEXT: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub name: String,
    pub title: String,
    pub description: String,
    pub vendor: String,
    pub kind: PackKind,
    /// The folders it holds, one skill or agent each.
    pub items: Vec<String>,
    pub license: String,
    /// An `https` page of where its items come from.
    #[serde(default)]
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackKind {
    Skills,
    Agents,
}

impl PackKind {
    /// The folder of the rules it goes under, and the file each item holds.
    pub fn place(self) -> (&'static str, &'static str) {
        match self {
            Self::Skills => ("skills", "SKILL.md"),
            Self::Agents => ("agents", "AGENT.md"),
        }
    }
}

impl Pack {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "{FILE} is over {MAX_BYTES} bytes");
        let pack: Self = serde_saphyr::from_slice(bytes).context("invalid pack.yml")?;
        ensure!(
            is_word(&pack.name, MAX_NAME),
            "the pack's name is not a-z, 0-9 and -"
        );
        ensure!(
            !pack.items.is_empty() && pack.items.len() <= MAX_ITEMS,
            "a pack holds 1 to {MAX_ITEMS} items"
        );
        let mut seen = BTreeSet::new();
        for item in &pack.items {
            ensure!(
                is_word(item, MAX_NAME),
                "the item {item:?} is not a-z, 0-9 and -"
            );
            ensure!(seen.insert(item), "the pack names {item} twice");
        }
        for text in [&pack.title, &pack.description, &pack.vendor, &pack.license] {
            ensure!(
                text.chars().count() <= MAX_TEXT && !text.chars().any(char::is_control),
                "a field of pack.yml is too long or holds control characters"
            );
        }
        if let Some(upstream) = &pack.upstream {
            ensure!(
                upstream.starts_with("https://") && upstream.len() <= 200,
                "the pack's upstream is not an https address"
            );
        }
        Ok(pack)
    }
}
