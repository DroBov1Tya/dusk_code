//! What an install would write, and what it changes in the copy installed here, asked
//! before an Unverified listing is installed or updated: nothing signed its list, so the
//! user looks at the files themselves, and the install names the hash of what they saw.
//! The files are fetched and checked as an install would, laid out in the staging folder
//! and removed; the old ones are read from disk, edits included.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use dusk_protocol::plugin::{FileRef, Kind, Review};

use super::{BUSY, Tier, content, fetch_package, record::Record, source};
use crate::{config::Mirror, init::paths::Paths, release::resources};

/// Far above any skill file; a bigger one on disk is shown as changed.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

pub async fn review(paths: &Paths, mirrors: &[Mirror], source: &str, name: &str) -> Result<Review> {
    let (fetched, failed) = source::read(paths, mirrors).await;
    if let Some(failure) = failed.iter().find(|item| item.source == source) {
        bail!("{}: {}", failure.source, failure.error);
    }
    let from = fetched
        .iter()
        .find(|item| item.name == source)
        .with_context(|| format!("there is no source named {source}"))?;
    let entry = from
        .index
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("{source} offers no {name}"))?;
    if entry.kind == Kind::Plugin {
        bail!("a plugin's files are on its Contents tab");
    }
    let (package, sha256) = fetch_package(entry).await?;
    let _busy = BUSY.lock().await;
    let unverified = from.tier == Tier::Unverified;
    let old = Record::load(paths)?
        .present(paths, name)
        .filter(|item| item.source == source)
        .map(|item| item.folder.clone());
    let (paths, entry) = (paths.clone(), entry.clone());
    tokio::task::spawn_blocking(move || {
        let files = match content::files_of(&paths, &entry, &package, unverified) {
            Ok(files) => files,
            Err(error) => {
                return Ok(Review {
                    package_sha256: sha256,
                    refused: Some(format!("{error:#}")),
                    ..Review::default()
                });
            }
        };
        let new = relative(&files.folder, files.wanted)?;
        let old = match &old {
            Some(folder) => on_disk(&paths.rules_root().join(folder))?,
            None => BTreeMap::new(),
        };
        Ok(compare(sha256, &old, &new))
    })
    .await
    .context("the review stopped")?
}

/// The files of an install by their path in its folder.
fn relative(folder: &str, wanted: BTreeMap<PathBuf, Vec<u8>>) -> Result<BTreeMap<String, Vec<u8>>> {
    wanted
        .into_iter()
        .map(|(path, bytes)| {
            let inside = path
                .strip_prefix(folder)
                .map_err(|_| anyhow!("{} is outside {folder}", path.display()))?;
            Ok((slashed(inside)?, bytes))
        })
        .collect()
}

/// The plain files under `folder`, links left out, by their path there.
fn on_disk(folder: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    if !folder.is_dir() {
        return Ok(files);
    }
    for path in resources::files(folder)? {
        let bytes = crate::init::files::read_regular(&folder.join(&path), MAX_FILE_BYTES)
            .unwrap_or_default();
        files.insert(slashed(&path)?, bytes);
    }
    Ok(files)
}

fn slashed(path: &Path) -> Result<String> {
    let parts: Option<Vec<&str>> = path.iter().map(|part| part.to_str()).collect();
    Ok(parts.context("a name that is not UTF-8")?.join("/"))
}

/// What the agent may run: a file of a script language, or one that starts with `#!`.
fn is_script(path: &str, bytes: &[u8]) -> bool {
    const LANGUAGES: [&str; 16] = [
        "sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "php", "ps1",
        "bat", "cmd", "lua",
    ];
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| LANGUAGES.contains(&extension)) || bytes.starts_with(b"#!")
}

/// The new files against the old, and the text diff of the first instruction file that
/// changed.
pub fn compare(
    package_sha256: String,
    old: &BTreeMap<String, Vec<u8>>,
    new: &BTreeMap<String, Vec<u8>>,
) -> Review {
    let mut review = Review {
        package_sha256,
        ..Review::default()
    };
    for (path, bytes) in new {
        review.files.push(FileRef {
            path: path.clone(),
            size: bytes.len() as u64,
            text: std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0),
        });
        match old.get(path) {
            None => review.added.push(path.clone()),
            Some(before) if before != bytes => review.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    review.scripts = new
        .iter()
        .filter(|(path, bytes)| is_script(path, bytes))
        .map(|(path, _)| path.clone())
        .collect();
    if let Some((_, text)) = new.iter().find(|(path, _)| path.ends_with("AGENT.md")) {
        review.agent = true;
        review.agent_tools =
            crate::catalog::graph::front_tools(&String::from_utf8_lossy(text)).flatten();
    }
    review.removed = old
        .keys()
        .filter(|path| !new.contains_key(*path))
        .cloned()
        .collect();
    let instructions = |path: &&String| path.ends_with("SKILL.md") || path.ends_with("AGENT.md");
    if let Some(path) = review.changed.iter().find(instructions).cloned() {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let (lines, cut) = crate::catalog::history::diff(&text(&old[&path]), &text(&new[&path]));
        review.diff = lines;
        review.diff_cut = cut;
        review.diff_path = Some(path);
    }
    review
}
