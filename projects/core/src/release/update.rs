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

use anyhow::{Context, Result, ensure};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{config::Config, init::paths::Paths};

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
    /// The release has the archive for this target; right after a tag it may still be
    /// building on GitHub.
    pub ready: bool,
}

#[derive(Deserialize)]
pub(super) struct Release {
    pub(super) tag_name: String,
    prerelease: bool,
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
}

impl Release {
    /// Both the archive for this target and its checksum are uploaded.
    pub(super) fn ready(&self) -> bool {
        let archive = format!("duskcode-{TARGET}.tar.gz");
        let checksum = format!("{archive}.sha256");
        [archive, checksum]
            .iter()
            .all(|name| self.assets.iter().any(|asset| &asset.name == name))
    }
}

#[derive(Deserialize)]
struct Comparison {
    ahead_by: u64,
}

/// The newest version on `channel`, and how far the running one is behind it.
pub async fn check(channel: Channel) -> Result<UpdateInfo> {
    let client = client()?;
    let release = newest(&client, channel).await?;
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
        ready: release.ready(),
        current,
        channel,
        latest: release.tag_name,
        prerelease: release.prerelease,
        commits_behind,
    })
}

/// Where the newest release of a channel is listed.
pub(super) fn releases_url(channel: Channel) -> String {
    let path = match channel {
        Channel::Stable => "releases/latest",
        Channel::Dev => "releases?per_page=20",
    };
    format!("{API}/{REPOSITORY}/{path}")
}

async fn newest(client: &Client, channel: Channel) -> Result<Release> {
    let response = client
        .get(releases_url(channel))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("cannot reach GitHub")?;
    if !response.status().is_success() {
        let error = refused(response).await;
        return Err(error.context(match channel {
            Channel::Stable => "cannot read the newest full release",
            Channel::Dev => "cannot read the releases",
        }));
    }
    newest_from(response, channel).await
}

/// The newest release in a successful answer to `releases_url`.
pub(super) async fn newest_from(response: reqwest::Response, channel: Channel) -> Result<Release> {
    Ok(match channel {
        Channel::Stable => response
            .json::<Release>()
            .await
            .context("unexpected answer from GitHub")?,
        Channel::Dev => response
            .json::<Vec<Release>>()
            .await
            .context("unexpected answer from GitHub")?
            .into_iter()
            .find(|release| !release.draft)
            .context("there are no releases yet")?,
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
    ensure!(
        info.ready,
        "{} is tagged, but its build for {TARGET} is not on GitHub yet; try again in a few minutes",
        info.latest
    );
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
    let archive = download(&client, &base).await.with_context(|| {
        format!(
            "cannot download the {tag} build for {TARGET}; right after a release is \
             tagged its builds take a few minutes on GitHub, so try again later"
        )
    })?;
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
    let work = directory.join(format!(".duskcode-update-{}", uuid::Uuid::now_v7()));
    let installed = unpack_into(&archive, &work, &name, &binary, tag.trim_start_matches('v'));
    // The work folder holds only what this update wrote.
    let _ = fs::remove_dir_all(&work);
    installed?;
    Ok(binary)
}

/// Unpacks the archive in `work` and moves its binary over `binary`, in the same folder
/// so the move is one rename.
fn unpack_into(
    archive: &[u8],
    work: &Path,
    name: &str,
    binary: &Path,
    version: &str,
) -> Result<()> {
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
    // If staging fails, the running binary and all existing snapshots stay intact.
    // A later binary-rename failure leaves only a harmless unused snapshot.
    crate::release::resources::install_snapshot(&work.join(name).join("resources"), version)?;
    fs::rename(&new, binary).context("cannot replace the binary")
}

pub(super) fn client() -> Result<Client> {
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
    if !response.status().is_success() {
        return Err(refused(response).await);
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
    if !response.status().is_success() {
        return Err(refused(response).await);
    }
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

/// Why GitHub said no, in words: a missing release, a used-up rate limit with the time
/// it lifts, or GitHub's own message, cut short.
pub(super) async fn refused(response: reqwest::Response) -> anyhow::Error {
    const MAX_MESSAGE: usize = 300;
    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let limited = header("x-ratelimit-remaining") == Some(0);
    let reset = header("x-ratelimit-reset");
    let message = response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|body| body.get("message")?.as_str().map(str::to_owned))
        .map(|message| message.chars().take(MAX_MESSAGE).collect::<String>());
    let code = status.as_u16();
    if limited || code == 429 {
        let wait = reset
            .and_then(|reset| reset.checked_sub(now_seconds()))
            .map_or("later".to_owned(), |seconds| {
                format!("in {} min", seconds.div_ceil(60).max(1))
            });
        return anyhow::anyhow!(
            "GitHub allows 60 requests an hour without a token and they are used up \
             ({status}); it answers again {wait}"
        );
    }
    let what = match code {
        404 => "not found: the release or its build is not on GitHub (yet)".to_owned(),
        403 => "access refused".to_owned(),
        500..=599 => "GitHub has a problem on its side; try again later".to_owned(),
        _ => "unexpected answer".to_owned(),
    };
    match message {
        Some(message) => anyhow::anyhow!("GitHub answered {status}, {what}: {message}"),
        None => anyhow::anyhow!("GitHub answered {status}, {what}"),
    }
}

pub(super) fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `v1.2.3` as numbers, for ordering; `None` for anything else.
pub(super) fn version(tag: &str) -> Option<(u64, u64, u64)> {
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
