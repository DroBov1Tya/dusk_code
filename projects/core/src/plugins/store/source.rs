//! Where listings come from: the official list, signed by the key built in, and the
//! mirrors the user added, each signed by its own pinned key. All are read at once; one
//! that fails is reported and the others are listed all the same.
//!
//! A listing's tier comes from the key that checked its bytes, never from what a list
//! says: only the official list is Official, and a mirror is Verified only when the
//! official list names it with the same address and key. A mirror without a key is read
//! unsigned and is Unverified whatever any list says; `guard` decides what it may bring.

use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::{Context, Result, anyhow, ensure};
use chrono::Utc;
use dusk_protocol::plugin::{
    Kind, LOCAL_SOURCE, OFFICIAL_SOURCE, Probe, SourceFailure, SourceRow, Tier,
};
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
    /// Base64 of the key that checked the list; empty for an Unverified source.
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
            Ok(mut index) => {
                let vouched = verified
                    .iter()
                    .any(|item| item.url == mirror.url && item.key == mirror.key);
                let tier = match (mirror.is_keyless(), vouched) {
                    // Nothing signed its list, whatever the official one names.
                    (true, _) => Tier::Unverified,
                    (false, true) => Tier::Verified,
                    (false, false) => Tier::Community,
                };
                if tier == Tier::Unverified {
                    super::guard::admit(&mut index);
                }
                fetched.push(Fetched {
                    name: mirror.name,
                    url: mirror.url,
                    key: mirror.key,
                    tier,
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
        // `remember` keeps no picture of an Unverified source, so its key, empty, only
        // has to stay empty.
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

/// What the list at `url` holds, checked with `key` exactly as a mirror's is, without
/// saving anything; with no key, read unsigned and kept to what an Unverified source may
/// bring. A GitHub repository's address is read as its Pages site's list.
pub async fn probe(url: &str, key: &str) -> Result<(Mirror, Index)> {
    let mirror = Mirror {
        name: "probe".to_owned(),
        url: list_address(url)?,
        key: key.trim().to_owned(),
    };
    mirror.check()?;
    let mut index = read_mirror(&mirror).await?;
    if mirror.is_keyless() {
        super::guard::admit(&mut index);
    }
    Ok((mirror, index))
}

/// What a probe tells the user before they add a repository.
pub fn probed(mirror: &Mirror, index: &Index) -> Probe {
    let count = |kind: Option<Kind>| {
        let entries = index.plugins.iter();
        let n = entries.filter(|entry| kind.is_none_or(|kind| entry.kind == kind));
        u32::try_from(n.count()).unwrap_or(u32::MAX)
    };
    Probe {
        url: mirror.url.clone(),
        entries: count(None),
        plugins: count(Some(Kind::Plugin)),
        serial: index.serial,
        expires: index.expires.clone(),
        unverified: mirror.is_keyless(),
        note: index.left_out.clone(),
    }
}

/// `https://github.com/<owner>/<repo>` becomes the list on that repository's Pages site,
/// where a Workshop repository publishes it; any other address is taken as the list's.
pub fn list_address(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/');
    let Some(path) = url.strip_prefix("https://github.com/") else {
        return Ok(url.to_owned());
    };
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    };
    ensure!(
        parts.len() == 2 && parts.iter().all(|part| plain(part)),
        "a GitHub address is https://github.com/<owner>/<repository>"
    );
    Ok(format!(
        "https://{}.github.io/{}/index.json",
        parts[0].to_ascii_lowercase(),
        parts[1]
    ))
}

/// A name for a new repository from its list's address: the folder the list is in, or
/// the host, as a word no other source has.
pub fn name_for(url: &str, taken: &[Mirror]) -> String {
    let parsed = reqwest::Url::parse(url).ok();
    let folder = parsed
        .as_ref()
        .and_then(|url| url.path_segments())
        .and_then(|segments| segments.rev().nth(1).map(str::to_owned))
        .filter(|folder| !folder.is_empty());
    let host = parsed.as_ref().and_then(|url| {
        url.host_str()
            .map(|host| host.split('.').next().unwrap_or("").to_owned())
    });
    let raw = folder.or(host).unwrap_or_default().to_ascii_lowercase();
    let word: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let base = word.trim_matches('-').chars().take(24).collect::<String>();
    let base = if base.is_empty() {
        "repository".to_owned()
    } else {
        base
    };
    let free = |name: &str| {
        name != OFFICIAL_SOURCE
            && name != LOCAL_SOURCE
            && !taken.iter().any(|mirror| mirror.name == name)
    };
    (1..)
        .map(|n| match n {
            1 => base.clone(),
            n => format!("{base}-{n}"),
        })
        .find(|name| free(name))
        .unwrap_or(base)
}

/// Each repository the user added, with how its list read this time.
pub fn rows(mirrors: &[Mirror], fetched: &[Fetched], failed: &[SourceFailure]) -> Vec<SourceRow> {
    mirrors
        .iter()
        .map(|mirror| {
            let read = fetched.iter().find(|item| item.name == mirror.name);
            let error = failed
                .iter()
                .find(|item| item.source == mirror.name)
                .map(|item| item.error.clone());
            SourceRow {
                name: mirror.name.clone(),
                url: mirror.url.clone(),
                fingerprint: match mirror.is_keyless() {
                    true => String::new(),
                    false => fingerprint(&mirror.key),
                },
                tier: match (read, mirror.is_keyless()) {
                    (Some(item), _) => item.tier,
                    (None, true) => Tier::Unverified,
                    (None, false) => Tier::Community,
                },
                entries: read.map_or(0, |item| {
                    u32::try_from(item.index.plugins.len()).unwrap_or(u32::MAX)
                }),
                error,
                note: read.and_then(|item| item.index.left_out.clone()),
            }
        })
        .collect()
}

/// The first 16 hex digits of the SHA-256 of a key, to tell keys apart at a glance.
fn fingerprint(key: &str) -> String {
    let mut hex = dusk_protocol::text::sha256_hex(key.as_bytes());
    hex.truncate(16);
    hex
}

/// A mirror's list, checked with its key; without one it is read unsigned, and its
/// signature is not even fetched.
async fn read_mirror(mirror: &Mirror) -> Result<Index> {
    if mirror.is_keyless() {
        let json = fetch(&mirror.url, index::MAX_INDEX_BYTES, Via::Direct)
            .await
            .context("cannot fetch its list")?;
        return index::parse(&json, Utc::now());
    }
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

/// Refuses a list older than one accepted from its source, and keeps a newer serial. An
/// unsigned list is not held to it: whoever serves it can write any serial, so the rule
/// would protect nothing and could lock the user out; the commit and file hashes the
/// record keeps of what it installed are what holds.
pub fn accept(paths: &Paths, fetched: &Fetched) -> Result<()> {
    if fetched.tier == Tier::Unverified {
        return Ok(());
    }
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
