//! A local connector that is a program from a GitHub release: one exact tag, one archive
//! per platform, each named by its SHA-256. Only the official list may bring one, under
//! its Official badge, since the program runs on this computer with the user's rights.
//! The archive comes from `github.com/<repository>/releases/download/<tag>/<file>` and
//! nowhere else; `install` checks and unpacks it.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::init::paths::Paths;

/// The largest archive a release connector may name or download.
pub const MAX_DOWNLOAD: u64 = 128 * 1024 * 1024;
const MAX_ASSETS: usize = 8;
const MAX_TAG: usize = 64;
const MAX_FILE: usize = 128;
const MAX_BINARY: usize = 200;

/// The platforms a release may name, as Rust's target triples. A program for another
/// one is refused rather than guessed at.
const TARGETS: [&str; 8] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
];

/// The `release` of a connector's server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    /// `owner/name` on github.com.
    pub repository: String,
    /// One exact tag, never `latest`.
    pub tag: String,
    /// One archive per target triple.
    pub assets: BTreeMap<String, Asset>,
}

/// One platform's archive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// The asset's file name in the release.
    pub file: String,
    /// Hex, lower case, of the archive.
    pub sha256: String,
    /// The program's path inside the archive.
    pub binary: String,
    /// The archive's size in bytes, when the list knows it; then it must match.
    pub size: Option<u64>,
}

/// The target triples this computer runs, best first: on Linux a static musl build runs
/// everywhere, a glibc one on most systems.
pub fn here() -> &'static [&'static str] {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["aarch64-apple-darwin"]
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        &["x86_64-apple-darwin"]
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        &["aarch64-unknown-linux-musl", "aarch64-unknown-linux-gnu"]
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"]
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        &["aarch64-pc-windows-msvc"]
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        &["x86_64-pc-windows-msvc"]
    } else {
        &[]
    }
}

impl Release {
    /// The archive for this computer, of the triples in `targets` in their order.
    pub fn asset_for(&self, targets: &[&str]) -> Option<&Asset> {
        targets.iter().find_map(|target| self.assets.get(*target))
    }

    /// The archive for this computer.
    pub fn asset(&self) -> Option<&Asset> {
        self.asset_for(here())
    }

    /// Where `asset` is downloaded from: GitHub's release download address, built from
    /// checked parts only.
    pub fn url(&self, asset: &Asset) -> String {
        format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.repository, self.tag, asset.file
        )
    }

    /// The program's file name, the last part of its path in the archive.
    pub fn program(asset: &Asset) -> &str {
        asset.binary.rsplit('/').next().unwrap_or(&asset.binary)
    }

    /// Where the program of `name` is put on this computer, versioned by tag:
    /// `<data>/connectors/<name>/<tag>/<program>`. None without a build for here.
    pub fn binary_path(&self, paths: &Paths, name: &str) -> Option<PathBuf> {
        let asset = self.asset()?;
        Some(
            folder(paths, name)
                .join(&self.tag)
                .join(Self::program(asset)),
        )
    }

    /// What the user agrees to before it is downloaded, in one line.
    pub fn consent(&self, name: &str) -> String {
        let Some(asset) = self.asset() else {
            return format!(
                "there is no build of {name} for this computer ({})",
                here().first().copied().unwrap_or("unknown")
            );
        };
        let size = match asset.size {
            Some(bytes) => format!("{:.1} MB", bytes as f64 / 1_000_000.0),
            None => "size not given".to_owned(),
        };
        format!(
            "download {} ({size}) from the release {} of github.com/{}, check that its SHA-256 starts with {}, put {} in duskcode's connectors folder, and run it on this computer with your rights while {name} is on",
            asset.file,
            self.tag,
            self.repository,
            &asset.sha256[..12.min(asset.sha256.len())],
            Self::program(asset),
        )
    }

    /// Why it cannot be added here, when it cannot.
    pub fn unfit(&self, name: &str) -> Option<String> {
        let Some(asset) = self.asset() else {
            return Some(format!(
                "{name} has no build for this computer ({})",
                here().first().copied().unwrap_or("unknown")
            ));
        };
        (!asset.file.ends_with(".tar.gz")).then(|| {
            format!(
                "its build for this computer is {}; duskcode unpacks .tar.gz only",
                asset.file
            )
        })
    }

    /// The rules of a list's entry.
    pub fn check(&self) -> Result<()> {
        ensure!(
            is_repository(&self.repository),
            "a release's repository is owner/name on github.com"
        );
        ensure!(
            is_tag(&self.tag),
            "a release names one exact tag of up to {MAX_TAG} plain characters, never latest"
        );
        ensure!(
            (1..=MAX_ASSETS).contains(&self.assets.len()),
            "a release names 1 to {MAX_ASSETS} archives"
        );
        for (target, asset) in &self.assets {
            ensure!(
                TARGETS.contains(&target.as_str()),
                "a release's platform {target} is not one duskcode knows"
            );
            let archive = match target.contains("windows") {
                true => ".zip",
                false => ".tar.gz",
            };
            ensure!(
                asset.file.len() <= MAX_FILE
                    && asset.file.ends_with(archive)
                    && asset.file.len() > archive.len()
                    && is_plain(&asset.file),
                "the archive for {target} is not a plain {archive} file name"
            );
            ensure!(
                super::super::index::is_sha256(&asset.sha256),
                "the archive {} has no SHA-256 in lower-case hex",
                asset.file
            );
            ensure!(
                asset
                    .size
                    .is_none_or(|size| (1..=MAX_DOWNLOAD).contains(&size)),
                "the archive {} is over {MAX_DOWNLOAD} bytes",
                asset.file
            );
            ensure!(
                asset.binary.len() <= MAX_BINARY && asset.binary.split('/').all(is_plain),
                "the program in {} is not a plain relative path",
                asset.file
            );
        }
        Ok(())
    }
}

/// Where every version of the connector `name` is kept.
pub fn folder(paths: &Paths, name: &str) -> PathBuf {
    paths.connectors_root().join(name)
}

/// The tag folder `binary` sits in, when it is a program the store put in place for
/// `name`: `<folder>/<tag>/<program>`.
pub fn tag_of(paths: &Paths, name: &str, binary: &Path) -> Option<String> {
    let rest = binary.strip_prefix(folder(paths, name)).ok()?;
    let parts: Vec<&str> = rest
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<_>>()?;
    let [tag, program] = parts.as_slice() else {
        return None;
    };
    (is_tag(tag) && is_plain(program)).then(|| (*tag).to_owned())
}

/// `owner/name` as GitHub allows them, nothing that reads as a path or another host.
fn is_repository(text: &str) -> bool {
    let Some((owner, name)) = text.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    owner_ok && name.len() <= 100 && is_plain(name)
}

/// A tag that is one plain folder name, not `latest`.
fn is_tag(text: &str) -> bool {
    text.len() <= MAX_TAG
        && is_plain(text)
        && !text.eq_ignore_ascii_case("latest")
        && !text.starts_with('-')
}

/// Letters, digits, `.`, `_`, `-` and `+`; not empty, not starting with a dot, so never
/// `.`, `..` or a hidden name.
fn is_plain(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}
