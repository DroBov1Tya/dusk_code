//! Whether the core may connect to an address on someone's word: a plugin's, a store
//! list's or the model's. Only public internet passes, never this machine, a private
//! network, a cloud's metadata service, or a tunnel that could lead to one of them.
//!
//! The rules are an allowlist: IPv6 must be global unicast (2000::/3) outside the
//! special blocks in it, and IPv4 outside every special-purpose block. An IPv4 address
//! carried in IPv6, mapped (::ffff:0:0/96) or translated by NAT64 (64:ff9b::/96), is
//! judged as the IPv4 address it carries; 6to4 (2002::/16) and Teredo are refused,
//! since the address they lead to is not the one checked.
//!
//! 198.18.0.0/15 is not public either: an address written as a number in it is refused.
//! Proxies with fake-IP DNS, common on desktops, answer every name with an address from
//! it and resolve the real one themselves, so a name whose addresses are all in that
//! range may be connected to: the proxy's tunnel catches them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Context, Result, ensure};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// The most addresses kept of a name; the rest are dropped.
const MAX_ADDRESSES: usize = 16;

/// An address on the public internet.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => match carried_v4(ip) {
            Some(v4) => public_v4(v4),
            None => public_v6(ip),
        },
    }
}

/// 198.18.0.0/15, where Clash and Surge put the addresses they answer in fake-IP mode.
pub fn is_fake_ip(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(ip) if matches!(ip.octets(), [198, 18 | 19, _, _]))
}

/// Whether the addresses `host` resolved to may be connected to: some, and all public,
/// or all a fake-IP proxy's when `host` is a name.
pub fn may_connect(host: &str, addresses: &[SocketAddr]) -> bool {
    let named = bare(host).parse::<IpAddr>().is_err();
    let all = |test: fn(IpAddr) -> bool| addresses.iter().all(|address| test(address.ip()));
    !addresses.is_empty() && (all(is_public) || named && all(is_fake_ip))
}

/// The addresses of `host` on `port`, unchecked, at most `MAX_ADDRESSES` of them; an
/// address written as a number is itself.
pub async fn lookup(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    Ok(tokio::net::lookup_host((bare(host), port))
        .await?
        .take(MAX_ADDRESSES)
        .collect())
}

/// Where `host` resolves to on `port`, when it may be connected to. The addresses are
/// the ones to connect to, pinned, so a second lookup cannot answer differently.
pub async fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let addresses = lookup(host, port)
        .await
        .context("cannot resolve the host")?;
    ensure!(!addresses.is_empty(), "the host has no address");
    ensure!(
        may_connect(host, &addresses),
        "the host resolves to an address on this machine or a private network"
    );
    Ok(addresses)
}

/// A resolver for reqwest that resolves a name only when it may be connected to, so a
/// list or a redirect cannot point the core at this machine, the local network or a
/// cloud's metadata service.
pub struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses = resolve(&host, 0).await.map_err(|_| {
                format!("{host} resolves to an address on this machine or a private network")
            })?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

/// A host without the brackets of an IPv6 address in a URL.
fn bare(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

/// The IPv4 address in a mapped (::ffff:0:0/96) or NAT64 (64:ff9b::/96) address.
fn carried_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return Some(mapped);
    }
    let segments = ip.segments();
    (segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0])
        .then(|| Ipv4Addr::from((u32::from(segments[6]) << 16) | u32::from(segments[7])))
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let special = matches!(a, 0 | 10 | 127 | 224..=255)
        // Shared address space, used behind carrier NAT.
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        // Protocol assignments, documentation, the 6to4 relay anycast and private.
        || (a == 192 && ((b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99) || b == 168))
        // Benchmarking (where fake-IP proxies answer) and documentation.
        || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113);
    !special
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    // Global unicast only: no loopback, link-local, unique local, multicast or others.
    (first & 0xe000) == 0x2000
        // IETF protocol assignments, Teredo among them, and documentation.
        && !(first == 0x2001 && (second < 0x0200 || second == 0x0db8))
        // 6to4, which carries an IPv4 address the tunnel leads to.
        && first != 0x2002
        // Documentation.
        && !(first == 0x3fff && second < 0x1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn this_machine_and_private_networks_are_not_public() {
        for address in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd00:ec2::254",
            "fec0::1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn multicast_and_reserved_blocks_are_not_public() {
        for address in [
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "ff02::1",
            "100::1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn documentation_and_protocol_blocks_are_not_public() {
        for address in [
            "192.0.0.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "2001:db8::1",
            "3fff::1",
            "3fff:fff::1",
            // IETF protocol assignments, with Teredo at 2001::/32.
            "2001::1",
            "2001:1ff::1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn the_fake_ip_range_is_not_public() {
        // 198.18.0.0/15, benchmarking, where fake-IP proxies answer.
        for address in ["198.18.0.1", "198.19.255.255"] {
            assert!(!public(address), "{address}");
            assert!(is_fake_ip(address.parse().unwrap()), "{address}");
        }
        assert!(public("198.17.255.255"));
        assert!(public("198.20.0.1"));
        assert!(!is_fake_ip("198.20.0.1".parse().unwrap()));
    }

    #[test]
    fn six_to_four_and_its_relay_are_not_public() {
        // 2002:7f00:1:: leads to 127.0.0.1, 2002:a00:1:: to 10.0.0.1; the gateway's own
        // address is not checked, so 6to4 is refused whatever it carries.
        for address in [
            "2002:7f00:1::",
            "2002:a00:1::",
            "2002:808:808::1",
            "192.88.99.1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn mapped_ipv4_is_judged_as_the_ipv4_it_carries() {
        for address in [
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:198.18.0.1",
        ] {
            assert!(!public(address), "{address}");
        }
        assert!(public("::ffff:8.8.8.8"));
    }

    #[test]
    fn nat64_is_judged_as_the_ipv4_it_carries() {
        for address in ["64:ff9b::a9fe:a9fe", "64:ff9b::7f00:1", "64:ff9b::a00:1"] {
            assert!(!public(address), "{address}");
        }
        assert!(public("64:ff9b::808:808"));
        // The local-use NAT64 prefix (64:ff9b:1::/48) leads to any network: refused.
        assert!(!public("64:ff9b:1::808:808"));
    }

    #[test]
    fn other_ways_to_write_an_ipv4_address_are_not_public() {
        for address in [
            // IPv4-compatible (deprecated), here 127.0.0.1.
            "::7f00:1",
            "::a00:1",
            // IPv4-translated (SIIT), here 127.0.0.1 and 8.8.8.8: refused whatever it carries.
            "::ffff:0:7f00:1",
            "::ffff:0:808:808",
            // NAT64 carrying a fake IP, 198.18.0.1.
            "64:ff9b::c612:1",
            // ORCHIDv2, inside the IETF protocol assignments.
            "2001:20::1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn public_addresses_pass() {
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "149.154.167.220",
            "2606:4700::1111",
            "2a00:1450:4001::1",
            "2001:4860:4860::8888",
        ] {
            assert!(public(address), "{address}");
        }
    }

    fn at(addresses: &[&str]) -> Vec<SocketAddr> {
        addresses
            .iter()
            .map(|address| SocketAddr::new(address.parse().unwrap(), 443))
            .collect()
    }

    #[test]
    fn a_fake_ip_name_passes_only_when_all_its_addresses_are_fake() {
        let fake = at(&["198.18.0.7"]);
        assert!(may_connect("example.com", &fake));
        // An address written as a number is what it says, not a proxy's answer.
        assert!(!may_connect("198.18.0.7", &fake));
        // A fake address beside a private one is no proxy's answer either.
        let mixed = at(&["198.18.0.7", "10.0.0.1"]);
        assert!(!may_connect("example.com", &mixed));
        assert!(!may_connect("example.com", &[]));
        assert!(may_connect("example.com", &at(&["8.8.8.8"])));
        assert!(!may_connect("example.com", &at(&["8.8.8.8", "127.0.0.1"])));
        // Only a plain IPv4 answer counts as fake: one carried in IPv6 is judged as it is.
        assert!(!may_connect("example.com", &at(&["::ffff:198.18.0.7"])));
        assert!(!may_connect("example.com", &at(&["64:ff9b::c612:7"])));
        // A mapped public address leads to that address and passes, as display_image takes it.
        assert!(may_connect("example.com", &at(&["::ffff:8.8.8.8"])));
    }

    #[tokio::test]
    async fn a_name_that_resolves_here_is_refused() {
        assert!(resolve("localhost", 443).await.is_err());
        assert!(resolve("[::1]", 443).await.is_err());
        assert!(resolve("198.18.0.1", 443).await.is_err());
        let name: Name = "localhost".parse().unwrap();
        assert!(PublicOnly.resolve(name).await.is_err());
    }
}
