//! Adding a source: one path for Settings (`StoreRequest::AddSource`) and for the agent's
//! `add_workshop_source`, which only proposes it to the owner. The list is read here
//! again whoever probed it before, so nothing a client or a model says is taken on trust.

use anyhow::{Result, ensure};

use super::{guard, index::Index, source};
use crate::config::{Mirror, is_word, live::LiveConfig};

/// Longest name of a source, as `config::workshop` takes it.
const MAX_NAME: usize = 32;

/// The source `url` with `key` (empty: Unverified), named `name` or after its address,
/// checked as `config.yml` would check it, before anything is fetched.
pub fn proposed(config: &LiveConfig, url: &str, key: &str, name: Option<&str>) -> Result<Mirror> {
    let mirrors = config.current().workshop().sources.clone();
    let url = source::list_address(url)?;
    let name = match name {
        Some(name) => {
            ensure!(
                is_word(name, MAX_NAME),
                "a source's name is up to {MAX_NAME} of a-z, 0-9 and -"
            );
            ensure!(
                !mirrors.iter().any(|item| item.name == name),
                "a source named {name} is added already"
            );
            name.to_owned()
        }
        None => source::name_for(&url, &mirrors),
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
