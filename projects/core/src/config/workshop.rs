//! The `workshop:` section: mirrors the user added beside the official store. A mirror is
//! read only with the key written here, so a mirror that changes its key stops being
//! read rather than asking whether to trust the new one.

use anyhow::{Result, ensure};
use base64::Engine;
use serde::{Deserialize, Serialize};

use dusk_protocol::plugin::{LOCAL_SOURCE, OFFICIAL_SOURCE};

/// Longest mirror name.
const MAX_NAME: usize = 32;
const MAX_URL: usize = 300;
/// Each is read on every look at the store.
const MAX_SOURCES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workshop {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Mirror>,
}

/// A signed list other than the official one: in `config.yml`, or named by the official
/// list as verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    /// `a-z0-9-`, shown as the listings' source.
    pub name: String,
    /// The `https` address of its `index.json`.
    pub url: String,
    /// Base64 of its Ed25519 public key; a list it does not sign is refused.
    pub key: String,
}

impl Workshop {
    pub fn is_default(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.sources.len() <= MAX_SOURCES,
            "workshop takes at most {MAX_SOURCES} sources"
        );
        let mut names = std::collections::BTreeSet::new();
        let mut urls = std::collections::BTreeSet::new();
        for mirror in &self.sources {
            mirror.check()?;
            ensure!(
                names.insert(&mirror.name) && urls.insert(&mirror.url),
                "workshop source {} is listed twice",
                mirror.name
            );
        }
        Ok(())
    }
}

impl Mirror {
    pub fn check(&self) -> Result<()> {
        ensure!(
            is_word(&self.name, MAX_NAME)
                && self.name != OFFICIAL_SOURCE
                && self.name != LOCAL_SOURCE,
            "workshop source name {:?} must be up to {MAX_NAME} of a-z, 0-9 and -, \
             and not official or local",
            self.name
        );
        let url = reqwest::Url::parse(&self.url);
        ensure!(
            self.url.len() <= MAX_URL
                && url.is_ok_and(|url| url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()),
            "workshop source {} must be a plain https address",
            self.name
        );
        let key = base64::engine::general_purpose::STANDARD.decode(&self.key);
        ensure!(
            key.is_ok_and(|key| key.len() == 32),
            "workshop source {} needs the base64 of a 32-byte Ed25519 key",
            self.name
        );
        Ok(())
    }
}

/// Lower case letters, digits and `-`, at most `max` of them: safe to show and to sort.
pub fn is_word(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}
