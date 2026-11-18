//! Where listings come from: the official list, signed by the key built in, and the
//! mirrors the user added, each signed by its own pinned key. All are read at once; one
//! that fails is reported and the others are listed all the same.
//!
//! A listing's tier comes from the key that checked its bytes, never from what a list
//! says: only the official list is Official, and a mirror is Verified only when the
//! official list names it with the same address and key.

use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::{Context, Result, anyhow, ensure};
use chrono::Utc;
use dusk_protocol::plugin::{OFFICIAL_SOURCE, SourceFailure, Tier};
use parking_lot::Mutex;

use super::{
    fetch::{Via, fetch},
    index::{self, Cover, Download, Index, Screenshot},
    record::Record,
};
use crate::{config::Mirror, init::paths::Paths};

/// The official list; the old address is tried when this one cannot be reached.
const OFFICIAL_URLS: [&str; 2] = [
    "https://drobov1tya.github.io/duskcode-workshop/index.json",
    "https://drobov1tya.github.io/duskcode-plugins/index.json",
];
/// Another official list, such as a fork's, for trying a store; it is still checked
/// against the key built in, so it cannot bring an unsigned plugin.
const INDEX_ENV: &str = "DUSKCODE_STORE_URL";

/// One source's list, checked.
#[derive(Debug)]
pub struct Fetched {
    pub name: String,
    /// The address its list came from; its pictures are beside it.
    pub url: String,
    /// Base64 of the key that checked the list.
    pub key: String,
    pub tier: Tier,
    pub index: Index,
}

/// The files of an entry as the last list read named them, its pictures and its package
/// for this machine, with where that list came from.
#[derive(Debug, Clone)]
pub struct Seen {
    pub url: String,
    pub key: String,
    pub cover: Option<Cover>,
    pub screenshots: Vec<Screenshot>,
    pub package: Option<Download>,
}

/// The entries of the lists read last, by (source, name), so a client asking for a
/// picture or the files of one gets only what a signed list named.
static SEEN: LazyLock<Mutex<BTreeMap<(String, String), Seen>>> = LazyLock::new(Mutex::default);

/// Reads the official list and every mirror. Each list must be no older than one
/// accepted here before.
pub async fn read(paths: &Paths, mirrors: &[Mirror]) -> (Vec<Fetched>, Vec<SourceFailure>) {
    let reads = futures::future::join_all(mirrors.iter().map(read_mirror));
    let (official, read) = futures::join!(read_official(), reads);
    let read = mirrors.iter().cloned().zip(read).collect();
    let (fetched, mut failed) = settle(Some(official), read);
    let paths = paths.clone();
    // The record is a file read and written under a lock; kept off the async threads.
    let accepted = tokio::task::spawn_blocking(move || {
        let mut kept = Vec::new();
        let mut failed = Vec::new();
        for item in fetched {
            match accept(&paths, &item) {
                Ok(()) => kept.push(item),
                Err(error) => failed.push(failure(&item.name, &error)),
            }
        }
        (kept, failed)
    })
    .await;
    let kept = match accepted {
        Ok((kept, refused)) => {
            failed.extend(refused);
            kept
        }
        Err(error) => {
            failed.push(failure(
                OFFICIAL_SOURCE,
                &anyhow!("the check stopped: {error}"),
            ));
            Vec::new()
        }
    };
    remember(&kept);
    (kept, failed)
}

/// Gives each list its tier. `official` is the list the key built in checked, with the
/// address it came from; `mirrors` are the user's, each checked with its own key.
pub fn settle(
    official: Option<Result<(String, Index)>>,
    mirrors: Vec<(Mirror, Result<Index>)>,
) -> (Vec<Fetched>, Vec<SourceFailure>) {
    let mut fetched = Vec::new();
    let mut failed = Vec::new();
    let verified = match &official {
        Some(Ok((_, index))) => index.verified.clone(),
        _ => Vec::new(),
    };
    match official {
        Some(Ok((url, index))) => fetched.push(Fetched {
            name: OFFICIAL_SOURCE.to_owned(),
            url,
            key: index::PUBLIC_KEY.to_owned(),
            tier: Tier::Official,
            index,
        }),
        Some(Err(error)) => failed.push(failure(OFFICIAL_SOURCE, &error)),
        None => {}
    }
    for (mirror, read) in mirrors {
        match read {
            Ok(index) => {
                let vouched = verified
                    .iter()
                    .any(|item| item.url == mirror.url && item.key == mirror.key);
                fetched.push(Fetched {
                    name: mirror.name,
                    url: mirror.url,
                    key: mirror.key,
                    tier: if vouched {
                        Tier::Verified
                    } else {
                        Tier::Community
                    },
                    index,
                });
            }
            Err(error) => failed.push(failure(&mirror.name, &error)),
        }
    }
    (fetched, failed)
}

/// What `name` of `source` showed in the last list read, while that source is still the
/// one configured: a mirror whose address or key changed since is not.
pub fn seen(mirrors: &[Mirror], source: &str, name: &str) -> Result<Seen> {
    let seen = SEEN
        .lock()
        .get(&(source.to_owned(), name.to_owned()))
        .cloned()
        .ok_or_else(|| anyhow!("{source} did not list {name}; list the store again"))?;
    let current = match source {
        OFFICIAL_SOURCE => seen.key == index::PUBLIC_KEY,
        _ => mirrors
            .iter()
            .any(|item| item.name == source && item.url == seen.url && item.key == seen.key),
    };
    ensure!(current, "the mirror {source} changed; list the store again");
    Ok(seen)
}

async fn read_official() -> Result<(String, Index)> {
    let urls = match std::env::var(INDEX_ENV) {
        Ok(url) => vec![url],
        Err(_) => OFFICIAL_URLS.map(str::to_owned).to_vec(),
    };
    let mut last = anyhow!("no address for the official list");
    for url in urls {
        // Only a list that cannot be reached falls back; one that arrives is checked
        // and its error stands.
        match fetch_signed(&url).await {
            Ok((json, signature)) => {
                let index = index::verify(&json, &signature, index::PUBLIC_KEY, Utc::now())?;
                return Ok((url, index));
            }
            Err(error) => {
                tracing::warn!("the official list at {url} cannot be read: {error:#}");
                last = error;
            }
        }
    }
    Err(last)
}

async fn read_mirror(mirror: &Mirror) -> Result<Index> {
    let (json, signature) = fetch_signed(&mirror.url).await?;
    index::verify(&json, &signature, &mirror.key, Utc::now())
}

/// The list and its signature; the error names no address, since it may reach a
/// browser, and the log keeps the rest.
async fn fetch_signed(url: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let json = fetch(url, index::MAX_INDEX_BYTES, Via::Direct)
        .await
        .context("cannot fetch its list")?;
    let signature = fetch(
        &format!("{url}.sig"),
        index::MAX_SIGNATURE_BYTES,
        Via::Direct,
    )
    .await
    .context("cannot fetch its list's signature")?;
    Ok((json, signature))
}

/// Refuses a list older than one accepted from its source, and keeps a newer serial.
fn accept(paths: &Paths, fetched: &Fetched) -> Result<()> {
    let serial = fetched.index.serial;
    Record::update(paths, |record| {
        let seen = match fetched.tier {
            Tier::Official => &mut record.serial,
            _ => record.mirrors.entry(fetched.url.clone()).or_default(),
        };
        ensure!(
            serial >= *seen,
            "it sent an older list ({serial}) than one seen here ({}); try again later",
            *seen
        );
        *seen = serial;
        Ok(())
    })
}

/// The screenshot at `index`, when the list named that many.
pub fn screenshot(seen: &Seen, index: u32) -> Option<&Screenshot> {
    usize::try_from(index)
        .ok()
        .and_then(|index| seen.screenshots.get(index))
}

pub fn remember(fetched: &[Fetched]) {
    let mut kept = SEEN.lock();
    for source in fetched {
        kept.retain(|(name, _), _| *name != source.name);
        for entry in &source.index.plugins {
            let seen = Seen {
                url: source.url.clone(),
                key: source.key.clone(),
                cover: entry.cover.clone(),
                screenshots: entry.screenshots.clone(),
                package: entry.download_here().ok().cloned(),
            };
            kept.insert((source.name.clone(), entry.name.clone()), seen);
        }
    }
}

/// The source and the outermost reason, which names no address; the whole chain goes
/// to the log.
fn failure(source: &str, error: &anyhow::Error) -> SourceFailure {
    tracing::warn!("store source {source}: {error:#}");
    SourceFailure {
        source: source.to_owned(),
        error: error.to_string(),
    }
}
