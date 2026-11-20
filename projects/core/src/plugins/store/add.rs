//! Adding a source: one path for Settings (`StoreRequest::AddSource`) and for the agent's
//! `add_workshop_source`, which only proposes it to the owner. The list is read here
//! again whoever probed it before, so nothing a client or a model says is taken on trust.

use anyhow::{Result, ensure};

use super::{guard, index::Index, source};
use crate::config::{Mirror, is_word, live::LiveConfig};

/// Longest name of a source, as `config::workshop` takes it.
const MAX_NAME: usize = 32;

/// The source `url` with `key` (empty: Unverified), checked as `config.yml` would check
/// it, before anything is fetched. Its name is the core's, from the address, as Settings
/// gives it; `suggested`, a name the model proposed, is taken only when it is a free name
/// made of words of the address itself and claims no trust, so a source at
/// `evil.example` cannot be shown as `anthropic-official`.
pub fn proposed(
    config: &LiveConfig,
    url: &str,
    key: &str,
    suggested: Option<&str>,
) -> Result<Mirror> {
    let mirrors = config.current().workshop().sources.clone();
    let url = source::list_address(url)?;
    let name = match suggested {
        Some(name) if fits(name, &url, &mirrors) => name.to_owned(),
        _ => source::name_for(&url, &mirrors),
    };
    let mirror = Mirror {
        name,
        url,
        key: key.trim().to_owned(),
    };
    mirror.check()?;
    // The address the store would refuse to fetch is refused before anyone is asked.
    super::fetch::check_url(&reqwest::Url::parse(&mirror.url)?)?;
    ensure!(
        !mirrors.iter().any(|item| item.url == mirror.url),
        "this repository is added already"
    );
    Ok(mirror)
}

/// Reads `mirror`'s list as the store would and, when it passes, saves the source.
/// Answers with its list as read.
pub async fn add(config: &LiveConfig, mirror: Mirror) -> Result<Index> {
    let (read, index) = source::probe(&mirror.url, &mirror.key).await?;
    ensure!(read.url == mirror.url, "the list's address changed");
    let mirrors = config.current().workshop().sources.clone();
    ensure!(
        !mirrors
            .iter()
            .any(|item| item.url == mirror.url || item.name == mirror.name),
        "this repository is added already"
    );
    tracing::info!("workshop repository {} added", mirror.name);
    config.update(|config| config.workshop_mut().sources.push(mirror))?;
    Ok(index)
}

/// Words a source's name may not use, since a listing shows it as where it comes from.
const CLAIMS: [&str; 5] = ["official", "verified", "duskcode", "local", "trusted"];

/// Whether a suggested name may stand: a free word whose every part is in the address.
fn fits(name: &str, url: &str, mirrors: &[Mirror]) -> bool {
    let address = url.to_ascii_lowercase();
    is_word(name, MAX_NAME)
        && !mirrors.iter().any(|item| item.name == name)
        && name
            .split('-')
            .all(|part| !part.is_empty() && !CLAIMS.contains(&part) && address.contains(part))
}

/// What the owner is asked before a tool adds `mirror`, in a few lines.
pub fn question(mirror: &Mirror) -> String {
    let trust = match mirror.is_keyless() {
        true => format!(
            "without a key, so it is Unverified: nobody signed its list. {}",
            guard::BRINGS
        ),
        false => "with its key; its items are Community".to_owned(),
    };
    format!(
        "add the Workshop source {} at {}, {trust}",
        mirror.name, mirror.url
    )
}
