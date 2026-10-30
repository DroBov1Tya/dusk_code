//! Looks for a newer release in the background, once a minute, on the channel the
//! config names. It asks with the ETag of the last answer, so an unchanged list costs
//! GitHub little, and it pauses until the rate limit lifts when few requests are left:
//! without a token GitHub allows 60 an hour for every program on this address.

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode, header};
use tracing::{debug, warn};

use super::update::{self, Channel, Release};
use crate::{
    config::live::LiveConfig,
    util::sync::{read, write},
};

const PERIOD: Duration = Duration::from_secs(60);
/// Requests kept for /update and other programs before the watch pauses.
const SPARE_REQUESTS: u64 = 10;

/// A newer release on the channel whose build is ready, as the last check found it.
#[derive(Default)]
pub struct UpdateWatch {
    found: RwLock<Option<String>>,
}

impl UpdateWatch {
    /// The newer version to offer, such as `v0.66.5`.
    pub fn found(&self) -> Option<String> {
        read(&self.found).clone()
    }

    pub async fn run(self: Arc<Self>, config: Arc<LiveConfig>) {
        let Ok(client) = update::client() else {
            return;
        };
        let mut etag: Option<(Channel, String)> = None;
        loop {
            let channel = config.current().update_channel();
            let known = etag
                .as_ref()
                .filter(|(was, _)| *was == channel)
                .map(|(_, tag)| tag.clone());
            let wait = match poll(&client, channel, known.as_deref()).await {
                Ok(polled) => {
                    if let Some(tag) = polled.etag {
                        etag = Some((channel, tag));
                    }
                    if let Some(release) = polled.release {
                        *write(&self.found) = offer(&release);
                    }
                    polled.wait.unwrap_or(PERIOD)
                }
                Err(error) => {
                    debug!("update check failed: {error:#}");
                    PERIOD
                }
            };
            tokio::time::sleep(wait).await;
        }
    }
}

struct Polled {
    /// `None` when nothing changed since the last answer.
    release: Option<Release>,
    etag: Option<String>,
    /// Longer than the period when the rate limit is nearly used up.
    wait: Option<Duration>,
}

fn offer(release: &Release) -> Option<String> {
    let current = format!("v{}", update::VERSION);
    let newer = update::version(&release.tag_name) > update::version(&current);
    (newer && release.ready()).then(|| release.tag_name.clone())
}

async fn poll(client: &Client, channel: Channel, etag: Option<&str>) -> Result<Polled> {
    let mut request = client
        .get(update::releases_url(channel))
        .header(header::ACCEPT, "application/vnd.github+json");
    if let Some(etag) = etag {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.context("cannot reach GitHub")?;
    let number = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let wait = match (number("x-ratelimit-remaining"), number("x-ratelimit-reset")) {
        (Some(left), Some(reset)) if left <= SPARE_REQUESTS => {
            let seconds = reset.saturating_sub(update::now_seconds()).max(60);
            warn!("few GitHub requests left; the update check waits {seconds} s");
            Some(Duration::from_secs(seconds))
        }
        _ => None,
    };
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(Polled {
            release: None,
            etag,
            wait,
        });
    }
    if !response.status().is_success() {
        return Err(update::refused(response).await);
    }
    let release = update::newest_from(response, channel).await?;
    Ok(Polled {
        release: Some(release),
        etag,
        wait,
    })
}
