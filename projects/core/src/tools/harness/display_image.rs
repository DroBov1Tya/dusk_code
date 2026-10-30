//! Deliberate, root-only image fetching. No message parser or URL detector calls this.

use std::{
    io::Cursor,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use reqwest::{
    Url,
    header::{ACCEPT, ACCEPT_ENCODING, LOCATION},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::tools::{
    Tool, ToolContext, ToolResult,
    builtin::{parse_arguments, printable},
};
use crate::{
    BoxFuture,
    state::artifacts::{ArtifactStore, ImageArtifact, MAX_IMAGE_BYTES, MAX_THUMBNAIL_BYTES},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 3;
const MAX_PIXELS: u64 = 4_000_000;
const MAX_DIMENSION: u32 = 8192;
const MAX_DECODE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DECODER_ALLOC: u64 = 64 * 1024 * 1024;
const THUMBNAIL_EDGE: u32 = 256;
/// Includes a detached decode after cancellation: a new call cannot start another
/// decoder until that bounded operation releases its permit.
static IMAGE_WORK: Semaphore = Semaphore::const_new(1);

pub struct DisplayImage {
    store: ArtifactStore,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    url: String,
    title: Option<String>,
    description: Option<String>,
}

struct Decoded {
    original: Vec<u8>,
    thumbnail: Vec<u8>,
    mime_type: &'static str,
    width: u32,
    height: u32,
}

impl DisplayImage {
    pub fn new(store: ArtifactStore) -> Self {
        Self { store }
    }

    async fn display(&self, arguments: Arguments) -> Result<ToolResult> {
        ensure!(arguments.url.len() <= 8192, "image URL is too long");
        let title = card_text(arguments.title, 160)?;
        let description = card_text(arguments.description, 2000)?;
        let work = async {
            let permit = IMAGE_WORK
                .acquire()
                .await
                .context("image decoder unavailable")?;
            let bytes = fetch(&arguments.url).await?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                decode(bytes)
            })
            .await
            .context("image decoder failed")?
        };
        let decoded = tokio::time::timeout(TIMEOUT, work)
            .await
            .context("image request exceeded 10 seconds")??;
        // No detached task writes artifacts. Dropping a cancelled fetch/decode future
        // discards its result; only a fully awaited success reaches this commit.
        let artifact = self.store.save(
            ImageArtifact {
                id: String::new(),
                title,
                description,
                mime_type: decoded.mime_type.into(),
                width: decoded.width,
                height: decoded.height,
                byte_len: 0,
                sha256: String::new(),
            },
            &decoded.original,
            &decoded.thumbnail,
        )?;
        Ok(ToolResult {
            content: format!(
                "Displayed image card {} ({}×{}, {}).",
                artifact.id, artifact.width, artifact.height, artifact.mime_type
            ),
            is_error: false,
            artifacts: vec![artifact],
        })
    }
}

impl Tool for DisplayImage {
    fn name(&self) -> &'static str {
        "display_image"
    }

    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Fetch a public HTTPS PNG or JPEG image and display a persistent image card. \
         Call deliberately when an image helps the user. Ordinary URLs in messages remain text \
         and are never fetched automatically. Optional title and description label the card. \
         Do not use private or authenticated URLs. Only the root agent has this tool."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({"type":"object", "additionalProperties":false,
            "properties": {
                "url":{"type":"string", "maxLength":8192, "description":"Public HTTPS image URL."},
                "title":{"type":"string", "maxLength":160},
                "description":{"type":"string", "maxLength":2000}
            }, "required":["url"]})
    }

    fn call<'a>(&'a self, _arguments: Value, _context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async { bail!("image tools require typed result transport") })
    }

    fn call_result<'a>(
        &'a self,
        arguments: Value,
        _context: ToolContext<'a>,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move { self.display(parse_arguments(arguments)?).await })
    }
}

fn card_text(value: Option<String>, max: usize) -> Result<Option<String>> {
    value
        .map(|value| {
            ensure!(value.chars().count() <= max, "image card text is too long");
            Ok(printable(&value))
        })
        .transpose()
}

fn checked_url(raw: &str) -> Result<Url> {
    // Parse errors and network errors intentionally omit URLs (queries may carry secrets).
    let url = Url::parse(raw).map_err(|_| anyhow::anyhow!("invalid image URL"))?;
    ensure!(url.scheme() == "https", "image URLs must use HTTPS");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "image URL credentials are forbidden"
    );
    ensure!(
        url.fragment().is_none(),
        "image URL fragments are forbidden"
    );
    ensure!(url.host_str().is_some(), "image URL has no host");
    ensure!(
        url.port_or_known_default() == Some(443),
        "image URLs must use port 443"
    );
    Ok(url)
}

async fn fetch(raw: &str) -> Result<Vec<u8>> {
    let mut url = checked_url(raw)?;
    for hop in 0..=MAX_REDIRECTS {
        let host = url.host_str().context("image URL has no host")?;
        let literal = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>();
        let named = literal.is_err();
        let addresses = match literal {
            Ok(ip) => vec![SocketAddr::new(ip, 443)],
            Err(_) => tokio::net::lookup_host((host, 443))
                .await
                .map_err(|_| anyhow::anyhow!("image host lookup failed"))?
                .take(17)
                .collect(),
        };
        ensure!(
            !addresses.is_empty() && addresses.len() <= 16,
            "image host has no bounded address set"
        );
        // A proxy in fake-ip mode answers every name from its own range and resolves
        // the real address itself. Such a name goes through the proxy from the
        // environment when one is set, else to the pinned fake address its TUN catches.
        let faked = named && addresses.iter().all(|address| fake_ip(address.ip()));
        ensure!(
            faked || addresses.iter().all(|address| public_ip(address.ip())),
            "image host is not public internet"
        );
        // Each redirect gets a fresh independent client. Resolution is pinned before
        // connect; TLS SNI/certificate verification still uses the original hostname.
        let mut client = reqwest::Client::builder();
        if !faked {
            client = client.no_proxy();
        }
        let client = client
            .https_only(true)
            // Wikimedia and others refuse requests that do not name their client.
            .user_agent(concat!("duskcode/", env!("CARGO_PKG_VERSION")))
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(TIMEOUT)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| anyhow::anyhow!("cannot initialize image client"))?;
        let mut response = client
            .get(url.clone())
            .header(ACCEPT, "image/png, image/jpeg")
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("image request failed"))?;
        if response.status().is_redirection() {
            ensure!(hop < MAX_REDIRECTS, "image redirect limit exceeded");
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .context("invalid image redirect")?;
            let next = url
                .join(location)
                .map_err(|_| anyhow::anyhow!("invalid image redirect"))?;
            url = checked_url(next.as_str())?;
            continue;
        }
        ensure!(
            response.status().is_success(),
            "image server returned HTTP {}",
            response.status().as_u16()
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|size| size <= MAX_IMAGE_BYTES as u64),
            "image exceeds 8 MiB"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("image download failed"))?
        {
            ensure!(
                chunk.len() <= MAX_IMAGE_BYTES - bytes.len(),
                "image exceeds 8 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        ensure!(!bytes.is_empty(), "image is empty");
        return Ok(bytes);
    }
    bail!("image redirect limit exceeded")
}

/// Conservative allowlist of global address space. Reject special-purpose ranges,
/// including IPv4-mapped IPv6, transition tunnels, documentation and reserved blocks.
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

/// 198.18.0.0/15, where Clash and Surge put the addresses they answer in fake-ip mode.
fn fake_ip(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(ip) if matches!(ip.octets(), [198, 18 | 19, _, _]))
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let special = matches!(a, 0 | 10 | 127 | 224..=255)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && ((b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99) || b == 168))
        || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113);
    !special
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    (segments[0] & 0xe000) == 0x2000
        && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
        && segments[0] != 0x2002
        && !(segments[0] == 0x3fff && segments[1] < 0x1000)
}

fn decode(original: Vec<u8>) -> Result<Decoded> {
    let format = image::guess_format(&original).context("unrecognized image content")?;
    let mime_type = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => bail!("only PNG and JPEG images are supported"),
    };
    let mut reader = ImageReader::with_format(Cursor::new(&original), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    reader.limits(limits);
    let decoder = reader.into_decoder().context("invalid image header")?;
    let (width, height) = decoder.dimensions();
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "image exceeds 4 megapixels"
    );
    ensure!(
        decoder.total_bytes() <= MAX_DECODE_BYTES,
        "decoded image exceeds 32 MiB"
    );
    let image = DynamicImage::from_decoder(decoder).context("invalid image content")?;
    let thumbnail = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).to_rgba8();
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(thumbnail)
        .write_to(&mut png, ImageFormat::Png)
        .context("cannot encode image thumbnail")?;
    ensure!(
        png.get_ref().len() <= MAX_THUMBNAIL_BYTES,
        "image thumbnail exceeds size limit"
    );
    Ok(Decoded {
        original,
        thumbnail: png.into_inner(),
        mime_type,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a name a fake-ip resolver answered may use that range; the address itself
    /// stays private.
    #[tokio::test]
    async fn a_fake_ip_address_by_itself_is_refused() {
        let error = fetch("https://198.18.0.1/logo.png").await.unwrap_err();
        assert_eq!(error.to_string(), "image host is not public internet");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn a_public_image_loads() {
        let bytes = fetch("https://www.python.org/static/img/python-logo.png")
            .await
            .unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
    }
}
