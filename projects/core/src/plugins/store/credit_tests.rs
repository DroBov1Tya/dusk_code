//! Format 3: who made an entry, and the tier it earns from that.

use dusk_protocol::plugin::Tier;

use super::{
    Agreed,
    index::{self, Index},
    listing, pick,
    source::Fetched,
    tests::{SHA, signed},
};

fn list(format: u32, entries: &str) -> String {
    format!(
        r#"{{"format": {format}, "serial": 3, "expires": "2030-01-01T00:00:00Z", "plugins": [{entries}]}}"#
    )
}

fn entry(name: &str, extra: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "downloads": [{{"url": "https://example.com/h.tar.gz", "sha256": "{SHA}", "size": 10}}]{extra}}}"#
    )
}

fn credit(author: &str, badge: &str) -> String {
    format!(
        r#", "author": "{author}", "license": "MIT", "source": "https://github.com/someone/x", "categories": ["coding", "review"], "badge": "{badge}""#
    )
}

fn read(json: &str) -> anyhow::Result<Index> {
    let (key, signature) = signed(json);
    let now = "2026-11-14T00:00:00Z".parse().unwrap();
    index::verify(json.as_bytes(), &signature, &key, now)
}

fn source(tier: Tier, entries: &str) -> Fetched {
    Fetched {
        name: "official".into(),
        url: "https://example.com/index.json".into(),
        key: String::new(),
        tier,
        index: read(&list(3, entries)).unwrap(),
    }
}

#[test]
fn a_format_3_list_says_who_made_each_entry() {
    let index = read(&list(3, &entry("hello", &credit("someone", "verified")))).unwrap();
    let hello = &index.plugins[0];
    assert_eq!(hello.author.as_deref(), Some("someone"));
    assert_eq!(hello.license.as_deref(), Some("MIT"));
    assert_eq!(
        hello.source.as_deref(),
        Some("https://github.com/someone/x")
    );
    assert_eq!(hello.categories, ["coding", "review"]);
}

#[test]
fn only_the_owners_work_in_the_official_list_is_official() {
    let entries = [
        entry("ours", &credit("Duskcode", "official")),
        entry("older", &credit("duskcode", "official")),
        entry("theirs", &credit("someone", "verified")),
        // The official list claiming Official for another author's work is lowered.
        entry("claimed", &credit("someone", "official")),
        entry("upstream", &credit("duskcode", "verified")),
    ]
    .join(", ");
    let official = source(Tier::Official, &entries);
    let tiers: Vec<Tier> = official
        .index
        .plugins
        .iter()
        .map(|item| listing::tier(&official, item))
        .collect();
    assert_eq!(
        tiers,
        [
            Tier::Official,
            Tier::Official,
            Tier::Verified,
            Tier::Verified,
            Tier::Verified
        ]
    );
    // A mirror's badge never raises it.
    for tier in [Tier::Verified, Tier::Community] {
        let mirror = source(tier, &entry("ours", &credit("duskcode", "official")));
        assert_eq!(listing::tier(&mirror, &mirror.index.plugins[0]), tier);
    }
}

#[test]
fn an_install_is_checked_against_the_entrys_own_tier() {
    let official = source(
        Tier::Official,
        &entry("theirs", &credit("someone", "verified")),
    );
    let fetched = [official];
    let agreed = |tier| Agreed {
        source: "official",
        tier,
        name: "theirs",
        version: "1.0.0",
        permissions: &[],
        sha256: None,
    };
    assert!(pick(&fetched, &agreed(Tier::Verified)).is_ok());
    assert!(pick(&fetched, &agreed(Tier::Official)).is_err());
}

#[test]
fn a_bad_format_3_entry_fails_the_list() {
    let good = credit("someone", "verified");
    let long = "a".repeat(65);
    let bad = [
        String::new(),
        good.replace(r#""author": "someone", "#, ""),
        good.replace(r#""license": "MIT", "#, ""),
        good.replace(r#", "badge": "verified""#, ""),
        good.replace(r#""categories": ["coding", "review"], "#, ""),
        good.replace("someone\"", &format!("{long}\"")),
        good.replace("someone\"", "some\\u202eone\""),
        good.replace("someone\"", "some\\none\""),
        good.replace("someone\"", " someone\""),
        good.replace("https://github", "http://github"),
        good.replace(r#"["coding", "review"]"#, "[]"),
        good.replace(r#""review""#, r#""games""#),
        good.replace(r#""review""#, r#""coding""#),
        good.replace(r#""review""#, r#""review", "data", "design""#),
        good.replace(r#""verified""#, r#""gold""#),
        format!("{good}, \"x\": 1"),
    ];
    for extra in bad {
        let json = list(3, &entry("hello", &extra));
        assert!(read(&json).is_err(), "{extra}");
    }
    assert!(read(&list(3, &entry("hello", &good))).is_ok());
}

#[test]
fn format_2_still_loads_and_keeps_format_3_fields_out() {
    let index = read(&list(2, &entry("hello", r#", "kind": "plugin""#))).unwrap();
    let official = Fetched {
        name: "official".into(),
        url: String::new(),
        key: String::new(),
        tier: Tier::Official,
        index,
    };
    assert_eq!(
        listing::tier(&official, &official.index.plugins[0]),
        Tier::Official
    );
    let json = list(2, &entry("hello", &credit("duskcode", "official")));
    assert!(read(&json).is_err());
    assert!(read(&list(4, &entry("hello", ""))).is_err());
}
