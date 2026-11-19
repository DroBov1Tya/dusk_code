//! Repositories the user adds: what they may claim, and where the core may go for them.

use dusk_protocol::plugin::Tier;

use super::{
    index, listing,
    source::{list_address, name_for, probe, settle},
    tests::{SHA, signed},
};
use crate::config::Mirror;

fn list(entries: &str) -> String {
    format!(
        r#"{{"format": 3, "serial": 1, "expires": "2030-01-01T00:00:00Z", "plugins": [{entries}]}}"#
    )
}

fn entry(name: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "kind": "skill", "downloads": [{{"os": "any", "arch": "any", "url": "https://example.com/h.tar.gz", "sha256": "{SHA}", "size": 10}}], "author": "duskcode", "license": "MIT", "source": "https://github.com/duskcode/x", "categories": ["coding"], "badge": "official"}}"#
    )
}

fn read(json: &str) -> anyhow::Result<(index::Index, String)> {
    let (key, signature) = signed(json);
    let now = "2026-11-14T00:00:00Z".parse().unwrap();
    index::verify(json.as_bytes(), &signature, &key, now).map(|index| (index, key))
}

#[test]
fn a_repository_claiming_official_stays_community() {
    let (index, key) = read(&list(&entry("x"))).unwrap();
    let mirror = Mirror {
        name: "friends".into(),
        url: "https://friends.example.com/index.json".into(),
        key,
    };
    let (fetched, failed) = settle(None, vec![(mirror, Ok(index))]);
    assert!(failed.is_empty());
    let source = &fetched[0];
    assert_eq!(source.tier, Tier::Community);
    assert_eq!(
        listing::tier(source, &source.index.plugins[0]),
        Tier::Community
    );
}

#[test]
fn a_list_past_the_entry_cap_is_refused_whole() {
    let entries: Vec<String> = (0..=index::MAX_ENTRIES)
        .map(|n| entry(&format!("s{n}")))
        .collect();
    let error = read(&list(&entries.join(","))).unwrap_err();
    assert!(error.to_string().contains("entries"), "{error:#}");
}

#[tokio::test]
async fn a_probe_refuses_http_and_private_addresses_before_any_fetch() {
    let key = read(&list("")).unwrap().1;
    for url in [
        "http://example.com/index.json",
        "https://127.0.0.1/index.json",
        "https://10.0.0.8/index.json",
        "https://[::1]/index.json",
        "https://169.254.169.254/latest/index.json",
        "https://user@example.com/index.json",
        "https://github.com/a/b/../c",
    ] {
        assert!(probe(url, &key).await.is_err(), "{url}");
    }
    assert!(
        probe("https://example.com/index.json", "not-a-key")
            .await
            .is_err()
    );
}

#[test]
fn a_github_repository_is_read_as_its_pages_list() {
    assert_eq!(
        list_address("https://github.com/Some-One/skills/").unwrap(),
        "https://some-one.github.io/skills/index.json"
    );
    assert_eq!(
        list_address("https://lists.example.com/w/index.json").unwrap(),
        "https://lists.example.com/w/index.json"
    );
    let taken = [Mirror {
        name: "skills".into(),
        url: "https://a.example.com/skills/index.json".into(),
        key: String::new(),
    }];
    assert_eq!(
        name_for("https://some-one.github.io/skills/index.json", &taken),
        "skills-2"
    );
    assert_eq!(
        name_for("https://official.example/index.json", &[]),
        "official-2"
    );
}
