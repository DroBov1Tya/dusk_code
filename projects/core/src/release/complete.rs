//! Resources are checked against the repository at the release's tag before they are
//! installed. A file the archive lacks, or one that differs from the tag, is fetched
//! from GitHub and checked against the hash git keeps for it, so nothing unverified is
//! written. Offline, the archive is used as it came.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail, ensure};
use futures::{StreamExt, TryStreamExt, stream};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use super::update::{REPOSITORY, client};

const API: &str = "https://api.github.com/repos";
const RAW: &str = "https://raw.githubusercontent.com";
/// Far above any prompt or skill file, so a wrong answer is not read without end.
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// The marker of the archive layout, not a resource to compare.
const SKIPPED: [&str; 1] = ["FORMAT"];
/// Files are small: a few at once hide the latency of each request.
const PARALLEL_FETCHES: usize = 8;
/// A resource is a few kilobytes; a request this slow is not going to finish.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Staging folders older than this were left by a process that was stopped.
const STALE_STAGING: Duration = Duration::from_secs(60 * 60);

/// What a restore is doing, for a client to show before its screen is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Asking GitHub which files the tag has.
    Listing,
    /// `done` of `total` files are fetched.
    Fetching { done: usize, total: usize },
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<Entry>,
    truncated: bool,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    r#type: String,
    sha: String,
}

/// Brings the resources in `source` to what the repository has at `v{version}`.
/// Returns how many files were fetched.
pub async fn complete(source: &Path, version: &str) -> Result<usize> {
    Ok(complete_with(source, version, &|_| {}).await?.0)
}

async fn complete_with(
    source: &Path,
    version: &str,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<(usize, HashSet<PathBuf>)> {
    let client = client()?;
    let tag = format!("v{version}");
    progress(Step::Listing);
    let url = format!("{API}/{REPOSITORY}/git/trees/{tag}?recursive=1");
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .context("cannot reach GitHub")?;
    ensure!(
        response.status().is_success(),
        "GitHub has no tree for {tag} ({})",
        response.status()
    );
    let tree: Tree = response
        .json()
        .await
        .context("unexpected answer from GitHub")?;
    ensure!(
        !tree.truncated,
        "the repository tree for {tag} is too large to check"
    );
    let blobs: Vec<&Entry> = tree
        .tree
        .iter()
        .filter(|entry| entry.r#type == "blob")
        .collect();
    // A build newer than its tag would fetch everything only to fail at the end.
    for required in super::resources::REQUIRED {
        let path = format!("resources/{required}");
        ensure!(
            blobs.iter().any(|entry| entry.path == path),
            "{tag} on GitHub lacks {required}, which this build needs"
        );
    }
    let mut wanted = Vec::new();
    let mut listed = HashSet::new();
    for entry in blobs {
        let Some(relative) = entry.path.strip_prefix("resources/") else {
            continue;
        };
        if SKIPPED.contains(&relative) {
            continue;
        }
        listed.insert(plain(relative)?);
        let target = source.join(plain(relative)?);
        let current = fs::read(&target).ok();
        if current.as_deref().map(git_hash).as_deref() != Some(entry.sha.as_str()) {
            wanted.push((entry, relative, target));
        }
    }
    let total = wanted.len();
    progress(Step::Fetching { done: 0, total });
    let done = std::sync::atomic::AtomicUsize::new(0);
    stream::iter(wanted)
        .map(|(entry, relative, target)| {
            let (client, tag, done) = (&client, &tag, &done);
            async move {
                let url = format!("{RAW}/{REPOSITORY}/{tag}/{}", entry.path);
                let bytes = fetch(client, &url).await?;
                ensure!(
                    git_hash(&bytes) == entry.sha,
                    "{} from GitHub does not match its hash; nothing was written",
                    entry.path
                );
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).context("cannot make a resource folder")?;
                }
                fs::write(&target, &bytes).with_context(|| format!("cannot write {relative}"))?;
                let done = done.fetch_add(1, Ordering::SeqCst) + 1;
                progress(Step::Fetching { done, total });
                Ok(())
            }
        })
        .buffer_unordered(PARALLEL_FETCHES)
        .try_collect::<()>()
        .await?;
    Ok((total, listed))
}

/// Makes sure the resources of this build are installed, fetching them from its tag
/// when they are not, as for a binary copied into place by hand. Tried once a process,
/// so a client that restores with its own progress is not followed by a second try.
/// Returns how many files were fetched, `None` when nothing had to be done.
pub async fn ensure_installed(progress: &(dyn Fn(Step) + Sync)) -> Result<Option<usize>> {
    static TRIED: AtomicBool = AtomicBool::new(false);
    if super::resources::root().is_ok() || TRIED.swap(true, Ordering::SeqCst) {
        return Ok(None);
    }
    restore(super::update::VERSION, progress).await.map(Some)
}

/// Builds the resources of `version` from its tag and installs them.
async fn restore(version: &str, progress: &(dyn Fn(Step) + Sync)) -> Result<usize> {
    let resources = super::resources::data_root()?.join("resources");
    crate::init::create_private_dir(&resources)?;
    remove_stale_staging(&resources);
    let staging = resources.join(format!(".restore-{}", uuid::Uuid::now_v7()));
    crate::init::create_private_dir(&staging)?;
    let restored = async {
        fs::write(staging.join("FORMAT"), "1\n").context("cannot write the resource format")?;
        fs::write(staging.join("VERSION"), format!("{version}\n"))
            .context("cannot write the resource version")?;
        seed(&resources, version, &staging);
        let (fetched, listed) = complete_with(&staging, version, progress).await?;
        // A file the older snapshot had and the tag dropped is not this version's.
        for relative in super::resources::files(&staging)? {
            let root_marker = relative.components().count() == 1;
            if !root_marker && !listed.contains(&relative) {
                fs::remove_file(staging.join(&relative))
                    .context("cannot drop a resource the release no longer has")?;
            }
        }
        super::resources::install_snapshot(&staging, version)?;
        Ok(fetched)
    }
    .await;
    // The staging folder holds only what this call wrote.
    let _ = fs::remove_dir_all(&staging);
    restored
}

/// Copies the newest installed snapshot of another version into `staging`, so only the
/// files that are missing or changed since are fetched. Without one, all are fetched.
fn seed(resources: &Path, version: &str, staging: &Path) {
    let newest = fs::read_dir(resources)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let key: Vec<u64> = name
                .split('.')
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            let directory = entry.file_type().ok()?.is_dir();
            (directory && name != version).then_some((key, entry.path()))
        })
        .max();
    let Some((_, snapshot)) = newest else {
        return;
    };
    let copied = super::resources::files(&snapshot).and_then(|files| {
        for relative in files.iter().filter(|path| path.components().count() > 1) {
            let to = staging.join(relative);
            crate::init::create_private_dir(to.parent().context("resource has no folder")?)?;
            fs::copy(snapshot.join(relative), to).context("cannot copy a resource")?;
        }
        Ok(())
    });
    if let Err(error) = copied {
        tracing::debug!("cannot start from {}: {error:#}", snapshot.display());
    }
}

/// A restore that was stopped leaves its staging folder; one untouched for an hour
/// belongs to no running process.
fn remove_stale_staging(resources: &Path) {
    let Ok(entries) = fs::read_dir(resources) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry.file_name().to_string_lossy().starts_with(".restore-")
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > STALE_STAGING);
        if stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .context("cannot reach GitHub")?;
    ensure!(response.status().is_success(), "cannot download {url}");
    let bytes = response.bytes().await.context("the download broke off")?;
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "{url} is too large for a resource"
    );
    Ok(bytes.to_vec())
}

/// A path from the tree as plain names only, so it cannot leave the resource folder.
fn plain(relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unexpected resource path {relative}");
    }
    Ok(path.to_owned())
}

/// The hash git gives a file's contents.
fn git_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()).as_bytes());
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_git_and_paths_stay_inside() {
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(
            git_hash(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert!(plain("skills/a/SKILL.md").is_ok());
        for bad in ["", "../x", "/etc/passwd", "a/../../b"] {
            assert!(plain(bad).is_err(), "{bad}");
        }
    }
}
