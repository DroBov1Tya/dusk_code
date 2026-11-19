//! Getting the store's files: over https only, to public addresses only, capped while
//! they arrive, and a package or cover kept only when its size and SHA-256 are the ones
//! its signed list names.
//!
//! Lists and covers follow no redirect: they sit where the list says. A package may be
//! redirected, as GitHub sends a release asset to its storage host, but only over https,
//! at most three times, and only to the host it was asked from or to GitHub's asset
//! hosts. Its SHA-256 is checked either way; the limits keep the core from being sent
//! to fetch from anywhere a list or a redirect names.

use std::{net::SocketAddr, sync::Arc, sync::LazyLock, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{
    Client, Url,
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::{Attempt, Policy},
};
use sha2::{Digest, Sha256};

use crate::{plugins::broker::is_public, release::update::VERSION};

const TIMEOUT: Duration = Duration::from_secs(120);
/// A list or a picture is small; a source that takes longer is reported, not waited on.
const DIRECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: usize = 3;
/// Where GitHub keeps release assets; a package link to a release is redirected there.
const PACKAGE_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// How a file is fetched: a list or cover without redirects, a package with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Direct,
    Package,
}

static DIRECT: LazyLock<Result<Client, String>> = LazyLock::new(|| client(Policy::none()));
static PACKAGE: LazyLock<Result<Client, String>> =
    LazyLock::new(|| client(Policy::custom(follow_package)));

/// The body at `url`, refused past `max` bytes.
pub async fn fetch(url: &str, max: u64, via: Via) -> Result<Vec<u8>> {
    check_url(&Url::parse(url).context("not a URL")?)?;
    let client = match via {
        Via::Direct => &*DIRECT,
        Via::Package => &*PACKAGE,
    };
    let client = client.as_ref().map_err(|error| anyhow!("{error}"))?;
    let timeout = match via {
        Via::Direct => DIRECT_TIMEOUT,
        Via::Package => TIMEOUT,
    };
    let mut response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .with_context(|| format!("cannot reach {url}"))?
        .error_for_status()
        .with_context(|| format!("the store refused {url}"))?;
    if response.content_length().is_some_and(|length| length > max) {
        bail!("{url} is over {max} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        body.extend_from_slice(&chunk);
        ensure!(body.len() as u64 <= max, "{url} is over {max} bytes");
    }
    Ok(body)
}

/// The file at `url`, when it has exactly `size` bytes and the SHA-256 `sha256`.
pub async fn fetch_checked(url: &str, size: u64, sha256: &str, via: Via) -> Result<Vec<u8>> {
    let body = fetch(url, size, via).await?;
    ensure!(
        body.len() as u64 == size,
        "{url} has {} bytes, the list says {size}",
        body.len()
    );
    ensure!(
        hex(&Sha256::digest(&body)) == sha256,
        "{url} is not the file the list names"
    );
    Ok(body)
}

/// `path`, already checked to be plain and relative, beside the list at `index_url`;
/// refused unless it stays in the list's folder.
pub fn beside(index_url: &str, path: &str) -> Result<String> {
    let base = reqwest::Url::parse(index_url).context("the list's address is not a URL")?;
    let url = base
        .join(path)
        .context("the cover's address is not valid")?;
    let folder = base
        .join("./")
        .context("the list's address has no folder")?;
    ensure!(
        url.scheme() == "https" && url.as_str().starts_with(folder.as_str()),
        "the cover is outside its list's folder"
    );
    Ok(url.into())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// https, and an address written as a number must be a public one: the resolver below
/// sees only names.
pub fn check_url(url: &Url) -> Result<()> {
    ensure!(url.scheme() == "https", "not an https address");
    let public = url.host_str().is_some_and(|host| {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        bare.parse::<std::net::IpAddr>().map_or(true, is_public)
    });
    ensure!(
        public,
        "the address is on this machine or a private network"
    );
    Ok(())
}

/// Whether a package fetched from `first` may be redirected to `next` after `hops`.
pub fn may_follow(first: &Url, next: &Url, hops: usize) -> bool {
    hops < MAX_REDIRECTS
        && check_url(next).is_ok()
        && next
            .host_str()
            .is_some_and(|host| Some(host) == first.host_str() || PACKAGE_HOSTS.contains(&host))
}

fn follow_package(attempt: Attempt) -> reqwest::redirect::Action {
    let previous = attempt.previous();
    let allowed = previous
        .first()
        .is_some_and(|first| may_follow(first, attempt.url(), previous.len()));
    match allowed {
        true => attempt.follow(),
        false => attempt.error("the package was redirected to a host the store does not allow"),
    }
}

fn client(redirects: Policy) -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .timeout(TIMEOUT)
        .redirect(redirects)
        .dns_resolver(Arc::new(PublicOnly))
        .user_agent(format!("duskcode/{VERSION}"))
        .build()
        .map_err(|error| format!("cannot start the store's client: {error}"))
}

/// Resolves a name only when every address it has is a public one, so a list cannot
/// point the core at this machine, the local network or a cloud's metadata service.
pub struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addresses.is_empty() || !addresses.iter().all(|address| is_public(address.ip())) {
                return Err(format!(
                    "{host} resolves to an address on this machine or a private network"
                )
                .into());
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
