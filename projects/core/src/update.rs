//! Updating the binary from the GitHub releases. The stable channel follows the full
//! releases, the round versions; the dev channel follows every tag, pre-releases too.
//! An update downloads the archive built for this target, checks it against its
//! published SHA-256, and moves the new binary over the running one, which goes on
//! until it is started again. Prompts and skills that ship with dusk are brought up to
//! date at that start, see `builtin`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{config::Config, paths::Paths};

const REPOSITORY: &str = "DroBov1Tya/dusk_code";
const API: &str = "https://api.github.com/repos";
const DOWNLOADS: &str = "https://github.com";
const TARGET: &str = env!("DUSK_TARGET");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Far above the size of a release archive, so a wrong file is not read without end.
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Dev,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Stable => Self::Dev,
            Self::Dev => Self::Stable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current: String,
    pub channel: Channel,
    /// The newest version on the channel, as its tag.
    pub latest: String,
    pub prerelease: bool,
    /// Commits from the running version to a newer `latest`, when GitHub knows both.
    pub commits_behind: Option<u64>,
    /// `latest` is newer than the running version.
    pub newer: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    draft: bool,
}

#[derive(Deserialize)]
struct Comparison {
    ahead_by: u64,
}

/// The newest version on `channel`, and how far the running one is behind it.
pub async fn check(channel: Channel) -> Result<UpdateInfo> {
    let client = client()?;
    let release = match channel {
        Channel::Stable => get_json::<Release>(&client, "releases/latest").await?,
        Channel::Dev => get_json::<Vec<Release>>(&client, "releases?per_page=20")
            .await?
            .into_iter()
            .find(|release| !release.draft)
            .context("there are no releases yet")?,
    };
    let current = format!("v{VERSION}");
    let newer = version(&release.tag_name) > version(&current);
    let compare = format!("compare/{current}...{}", release.tag_name);
    // A build of an untagged commit has nothing to compare from.
    let commits_behind = match newer {
        true => get_json::<Comparison>(&client, &compare)
            .await
            .ok()
            .map(|comparison| comparison.ahead_by),
        false => None,
    };
    Ok(UpdateInfo {
        newer,
        current,
        channel,
        latest: release.tag_name,
        prerelease: release.prerelease,
        commits_behind,
    })
}

/// `duskcode --update`: installs the newest version on the channel in config.yml, if it
/// is newer. Returns what happened, for the terminal.
pub async fn update_now(paths: &Paths) -> Result<String> {
    let channel = match fs::read(paths.config_file()) {
        Ok(bytes) => Config::parse(&bytes)?.update_channel(),
        Err(_) => Channel::default(),
    };
    let info = check(channel).await?;
    if !info.newer {
        return Ok(format!(
            "{} is up to date on the {} channel (newest {})",
            info.current,
            channel.name(),
            info.latest
        ));
    }
    let binary = install(&info.latest).await?;
    Ok(format!(
        "updated {} to {} in {}",
        info.current,
        info.latest,
        binary.display()
    ))
}

/// Downloads `tag` for this target and puts it in place of the running binary. Returns
/// where the binary is.
pub async fn install(tag: &str) -> Result<PathBuf> {
    ensure!(
        tag.starts_with('v') && version(tag).is_some(),
        "invalid version {tag}"
    );
    ensure!(!TARGET.is_empty(), "this build does not know its target");
    let client = client()?;
    let name = format!("duskcode-{TARGET}");
    let base = format!("{DOWNLOADS}/{REPOSITORY}/releases/download/{tag}/{name}.tar.gz");
    let archive = download(&client, &base)
        .await
        .with_context(|| format!("no {tag} build for {TARGET}"))?;
    let published = download(&client, &format!("{base}.sha256")).await?;
    let published = String::from_utf8_lossy(&published);
    let expected = published.split_whitespace().next().unwrap_or_default();
    ensure!(
        hex(&Sha256::digest(&archive)) == expected.to_ascii_lowercase(),
        "the download does not match its checksum; nothing was changed"
    );
    let binary = std::env::current_exe()
        .and_then(fs::canonicalize)
        .context("cannot find the running binary")?;
    let directory = binary.parent().context("the binary has no folder")?;
    let work = directory.join(format!(".duskcode-update-{}", std::process::id()));
    let installed = unpack_into(&archive, &work, &name, &binary);
    // The work folder holds only what this update wrote.
    let _ = fs::remove_dir_all(&work);
    installed?;
    Ok(binary)
}

/// Unpacks the archive in `work` and moves its binary over `binary`, in the same folder
/// so the move is one rename.
fn unpack_into(archive: &[u8], work: &Path, name: &str, binary: &Path) -> Result<()> {
    fs::create_dir(work).context("cannot write next to the binary")?;
    let file = work.join("archive.tar.gz");
    fs::write(&file, archive).context("cannot write the download")?;
    let unpacked = Command::new("tar")
        .arg("-xzf")
        .arg(&file)
        .arg("-C")
        .arg(work)
        .status()
        .context("tar is needed to unpack the update")?;
    ensure!(unpacked.success(), "cannot unpack the download");
    let new = work.join(name).join("duskcode");
    ensure!(new.is_file(), "the download has no duskcode binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&new, fs::Permissions::from_mode(0o755))
            .context("cannot make the new binary runnable")?;
    }
    fs::rename(&new, binary).context("cannot replace the binary")
}

fn client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("duskcode/{VERSION}"))
        .timeout(TIMEOUT)
        .https_only(true)
        .build()
        .context("cannot set up HTTPS")
}

async fn get_json<T: serde::de::DeserializeOwned>(client: &Client, path: &str) -> Result<T> {
    let url = format!("{API}/{REPOSITORY}/{path}");
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("cannot reach GitHub")?;
    let status = response.status();
    if !status.is_success() {
        bail!("GitHub answered {status}");
    }
    response
        .json()
        .await
        .context("unexpected answer from GitHub")
}

async fn download(client: &Client, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .send()
        .await
        .context("cannot reach GitHub")?;
    ensure!(
        response.status().is_success(),
        "GitHub answered {}",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.context("the download broke off")? {
        bytes.extend_from_slice(&chunk);
        ensure!(
            bytes.len() <= MAX_ARCHIVE_BYTES,
            "the download is too large"
        );
    }
    Ok(bytes)
}

/// `v1.2.3` as numbers, for ordering; `None` for anything else.
fn version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.').map(str::parse);
    let version = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_by_number() {
        assert!(version("v0.60.0") > version("v0.9.9"));
        assert_eq!(version("v1.2"), None);
        assert_eq!(version("1.2.3"), None);
        assert_eq!(version("v1.2.3.4"), None);
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }
}
