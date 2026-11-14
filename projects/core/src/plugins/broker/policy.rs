//! What a plugin's broker lets out: the origins it may reach, the public addresses they
//! resolve to, and the keys it puts into requests for the origins each was given for.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::client::Secret;
use reqwest::Url;

const KEY_OPEN: &str = "{{key:";
const KEY_CLOSE: &str = "}}";

/// Scheme, host and port: where a request goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub(super) scheme: String,
    pub(super) host: String,
    pub(super) port: u16,
}

impl Origin {
    /// An origin as a manifest names it: `https://host[:port]`, nothing more.
    pub fn parse(text: &str) -> Result<Self> {
        let url = Url::parse(text).with_context(|| format!("{text} is not an origin"))?;
        ensure!(
            url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
            "{text} is not an origin: no path, query or fragment"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "{text} is not an origin: no user or password"
        );
        let origin = Self::of(&url)?;
        ensure!(
            origin.scheme == "https",
            "{text}: a plugin reaches https origins only"
        );
        Ok(origin)
    }

    pub(super) fn of(url: &Url) -> Result<Self> {
        Ok(Self {
            scheme: url.scheme().to_owned(),
            host: url
                .host_str()
                .context("the target has no host")?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_ascii_lowercase(),
            port: url
                .port_or_known_default()
                .context("the target has no port")?,
        })
    }
}

/// One of the plugin's keys, with the origins it may go to.
pub struct Key {
    pub name: String,
    pub value: Secret,
    pub origins: Vec<Origin>,
}

pub struct Policy {
    /// What the plugin's programs prove they are with.
    pub token: Secret,
    pub origins: Vec<Origin>,
    pub keys: Vec<Key>,
    /// Off only in tests, which serve on this machine.
    pub(super) public_only: bool,
}

impl Policy {
    pub fn new(token: Secret, origins: Vec<Origin>, keys: Vec<Key>) -> Self {
        Self {
            token,
            origins,
            keys,
            public_only: true,
        }
    }

    pub(super) fn allows(&self, origin: &Origin) -> bool {
        self.origins.contains(origin)
    }

    /// Where `host` resolves to, when every address is a public one.
    pub(super) async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .context("cannot resolve the host")?
            .collect();
        ensure!(!addresses.is_empty(), "the host has no address");
        if self.public_only && !addresses.iter().all(|address| is_public(address.ip())) {
            bail!("the host resolves to an address on this machine or a private network");
        }
        Ok(addresses)
    }

    /// `text` with each `{{key:name}}` replaced by the key, when `origin` is one the key
    /// was given for. The values put in, to keep them out of the answer.
    pub(super) fn put_keys(&self, text: &str, origin: &Origin) -> Result<(String, Vec<&Secret>)> {
        let mut out = String::with_capacity(text.len());
        let mut used = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find(KEY_OPEN) {
            out.push_str(&rest[..start]);
            let after = &rest[start + KEY_OPEN.len()..];
            let end = after
                .find(KEY_CLOSE)
                .context("a key placeholder is not closed")?;
            let name = &after[..end];
            let key = self
                .keys
                .iter()
                .find(|key| key.name == name)
                .with_context(|| format!("the plugin has no key {name}"))?;
            ensure!(
                key.origins.contains(origin),
                "the key {name} is not for this origin"
            );
            out.push_str(&key.value.0);
            used.push(&key.value);
            rest = &after[end + KEY_CLOSE.len()..];
        }
        out.push_str(rest);
        Ok((out, used))
    }
}

/// An address on the internet: not this machine, a private or link-local network, a
/// multicast group or a cloud's metadata service.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_public_v4(mapped),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || a == 0
        // Shared address space, used behind carrier NAT.
        || (a == 100 && (64..128).contains(&b))
        // Protocol assignments. 198.18.0.0/15 stays: no service lives there, and proxies
        // with fake-IP DNS, common on desktops, answer every name with one of it.
        || (a == 192 && b == 0 && ip.octets()[2] == 0)
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // NAT64 carries an IPv4 address in its last 32 bits.
    if ip.segments()[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [.., c, d] = ip.segments();
        let v4 = Ipv4Addr::from((u32::from(c) << 16) | u32::from(d));
        return is_public_v4(v4);
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique local, link-local and the old site-local.
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xffc0) == 0xfec0
        // Documentation.
        || (first == 0x2001 && ip.segments()[1] == 0x0db8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_https_origin_is_one() {
        let origin = Origin::parse("https://API.telegram.org").unwrap();
        assert_eq!(origin.host, "api.telegram.org");
        assert_eq!(origin.port, 443);
        for wrong in [
            "http://api.telegram.org",
            "https://api.telegram.org/bot",
            "https://user:pass@api.telegram.org",
            "https://api.telegram.org?x=1",
            "api.telegram.org",
        ] {
            assert!(Origin::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn private_and_metadata_addresses_are_not_public() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd00:ec2::254",
            "::ffff:127.0.0.1",
            "64:ff9b::a9fe:a9fe",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["1.1.1.1", "149.154.167.220", "2606:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn a_key_goes_only_to_its_origin() {
        let telegram = Origin::parse("https://api.telegram.org").unwrap();
        let other = Origin::parse("https://example.com").unwrap();
        let policy = Policy::new(
            Secret("capability".into()),
            vec![telegram.clone(), other.clone()],
            vec![Key {
                name: "token".into(),
                value: Secret("123:abc".into()),
                origins: vec![telegram.clone()],
            }],
        );
        let (text, used) = policy
            .put_keys("/bot{{key:token}}/getMe", &telegram)
            .unwrap();
        assert_eq!(text, "/bot123:abc/getMe");
        assert_eq!(used.len(), 1);
        assert!(policy.put_keys("/{{key:token}}", &other).is_err());
        assert!(policy.put_keys("/{{key:other}}", &telegram).is_err());
        assert!(policy.put_keys("/{{key:token", &telegram).is_err());
    }
}
