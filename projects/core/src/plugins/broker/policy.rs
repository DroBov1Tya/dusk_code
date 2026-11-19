//! What a plugin's broker lets out: the origins it may reach, the public addresses they
//! resolve to, and the keys it puts into requests for the origins each was given for.

use std::net::SocketAddr;

use anyhow::{Context, Result, ensure};
use dusk_protocol::client::Secret;
use reqwest::Url;

use crate::net::public;

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

    /// Where `host` resolves to, when every address is a public one. A name a fake-IP
    /// proxy answered passes: the addresses are pinned, and its tunnel catches them.
    pub(super) async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        if self.public_only {
            return public::resolve(host, port).await;
        }
        let addresses = public::lookup(host, port)
            .await
            .context("cannot resolve the host")?;
        ensure!(!addresses.is_empty(), "the host has no address");
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
