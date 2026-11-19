//! Skills, packs and agents fetched straight from their author's repository instead of a package:
//! the list pins a commit and the SHA-256 of every file, and the core fetches exactly
//! those files at that commit from GitHub's raw host. Every file is checked before
//! anything is written, and a missing, extra or changed file refuses the install.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::fetch::{self, Via};

/// The one host files are fetched from; a list cannot name another.
const HOST: &str = "https://raw.githubusercontent.com";
pub const MAX_FILES: usize = 200;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PATH: usize = 300;

/// Where an entry's files are, as its list names them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// `owner/name` on GitHub.
    pub repo: String,
    /// 40 hex digits, lower case.
    pub commit: String,
    pub files: Vec<UpstreamFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamFile {
    /// In the repository.
    pub path: String,
    /// In the item's folder.
    pub to: String,
    pub sha256: String,
    pub size: u64,
}

impl Upstream {
    pub fn check(&self) -> Result<()> {
        let parts: Vec<&str> = self.repo.split('/').collect();
        ensure!(
            parts.len() == 2 && parts.iter().all(|part| plain_part(part)),
            "the upstream repository is not owner/name"
        );
        ensure!(
            self.commit.len() == 40
                && self
                    .commit
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
            "the upstream commit is not 40 hex digits"
        );
        ensure!(
            (1..=MAX_FILES).contains(&self.files.len()),
            "an upstream names 1 to {MAX_FILES} files"
        );
        let mut paths = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for file in &self.files {
            ensure!(
                plain_path(&file.path) && plain_path(&file.to),
                "an upstream file's path is not a plain relative path"
            );
            ensure!(
                paths.insert(&file.path) && targets.insert(&file.to),
                "an upstream file is named twice"
            );
            ensure!(
                file.sha256.len() == 64
                    && file
                        .sha256
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
                "an upstream file's sha256 is not 64 hex digits"
            );
            ensure!(
                (1..=MAX_FILE_BYTES).contains(&file.size),
                "an upstream file's size is not 1 to {MAX_FILE_BYTES} bytes"
            );
        }
        Ok(())
    }

    /// `github.com/owner/name at abc1234`, for the user.
    pub fn line(&self) -> String {
        format!(
            "github.com/{} at {}",
            self.repo,
            self.commit.get(..7).unwrap_or(&self.commit)
        )
    }

    /// One hash for the files the list pins, kept in the record as a package's is.
    pub fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.commit.as_bytes());
        for file in &self.files {
            hash.update(format!("\n{} {} {}", file.to, file.sha256, file.size).as_bytes());
        }
        fetch::hex(&hash.finalize())
    }
}

/// Every file of `upstream`, fetched and checked, by its `to`.
pub async fn fetch_all(upstream: &Upstream) -> Result<BTreeMap<String, Vec<u8>>> {
    upstream.check()?;
    let mut fetched = Vec::new();
    for file in &upstream.files {
        let url = format!("{HOST}/{}/{}/{}", upstream.repo, upstream.commit, file.path);
        // Read to the cap; `assemble` checks size and hash, so one place refuses.
        let body = fetch::fetch(&url, file.size, Via::Direct)
            .await
            .with_context(|| format!("cannot fetch {} from upstream", file.path))?;
        fetched.push((file.path.clone(), body));
    }
    assemble(upstream, fetched)
}

/// The files by their `to`, when `fetched` holds exactly the files `upstream` names, each
/// of its size and hash, as UTF-8 text.
pub fn assemble(
    upstream: &Upstream,
    fetched: Vec<(String, Vec<u8>)>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut by_path: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, body) in fetched {
        ensure!(
            by_path.insert(path.clone(), body).is_none(),
            "{path} came twice"
        );
    }
    let mut files = BTreeMap::new();
    for file in &upstream.files {
        let body = by_path
            .remove(&file.path)
            .with_context(|| format!("{} is missing", file.path))?;
        ensure!(
            body.len() as u64 == file.size && fetch::hex(&Sha256::digest(&body)) == file.sha256,
            "{} is not the file the list pins",
            file.path
        );
        ensure!(
            std::str::from_utf8(&body).is_ok() && !body.contains(&0),
            "{} is not text",
            file.path
        );
        files.insert(file.to.clone(), body);
    }
    if let Some(extra) = by_path.keys().next() {
        anyhow::bail!("{extra} is not a file the list names");
    }
    Ok(files)
}

fn plain_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn plain_path(path: &str) -> bool {
    !path.is_empty() && path.len() <= MAX_PATH && path.split('/').all(plain_part)
}
