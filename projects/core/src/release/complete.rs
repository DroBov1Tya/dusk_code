//! Resources are checked against the repository at the release's tag before they are
//! installed. A file the archive lacks, or one that differs from the tag, is fetched
//! from GitHub and checked against the hash git keeps for it, so nothing unverified is
//! written. Offline, the archive is used as it came.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use super::update::{REPOSITORY, client};

const API: &str = "https://api.github.com/repos";
const RAW: &str = "https://raw.githubusercontent.com";
/// Far above any prompt or skill file, so a wrong answer is not read without end.
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// The marker of the archive layout, not a resource to compare.
const SKIPPED: [&str; 1] = ["FORMAT"];

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
    let client = client()?;
    let tag = format!("v{version}");
    let url = format!("{API}/{REPOSITORY}/git/trees/{tag}?recursive=1");
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
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
    let mut fetched = 0;
    for entry in tree.tree.iter().filter(|entry| entry.r#type == "blob") {
        let Some(relative) = entry.path.strip_prefix("resources/") else {
            continue;
        };
        if SKIPPED.contains(&relative) {
            continue;
        }
        let target = source.join(plain(relative)?);
        let current = fs::read(&target).ok();
        if current.as_deref().map(git_hash).as_deref() == Some(entry.sha.as_str()) {
            continue;
        }
        let bytes = fetch(&client, &format!("{RAW}/{REPOSITORY}/{tag}/{}", entry.path)).await?;
        ensure!(
            git_hash(&bytes) == entry.sha,
            "{} from GitHub does not match its hash; nothing was written",
            entry.path
        );
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).context("cannot make a resource folder")?;
        }
        fs::write(&target, &bytes).with_context(|| format!("cannot write {relative}"))?;
        fetched += 1;
    }
    Ok(fetched)
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
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
