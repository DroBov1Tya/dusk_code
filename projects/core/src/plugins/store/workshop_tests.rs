//! Format 2, sources and tiers, covers, updates.

use anyhow::anyhow;
use dusk_protocol::plugin::{Kind, Listing, Order, RepoStars, Tier};
use proptest::prelude::*;

use super::{
    asks_more, fetch,
    index::{self, Cover, Index},
    listing::{self, newer, picks, sort},
    record::Record,
    source::settle,
    tests::{SHA, signed},
};
use crate::config::{Mirror, Workshop};

fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-11-14T00:00:00Z".parse().unwrap()
}

fn list(format: u32, entries: &str, verified: &str) -> String {
    format!(
        r#"{{"format": {format}, "serial": 3, "expires": "2030-01-01T00:00:00Z", "plugins": [{entries}]{verified}}}"#
    )
}

fn entry(name: &str, extra: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "downloads": [{{"url": "https://example.com/h.tar.gz", "sha256": "{SHA}", "size": 10}}]{extra}}}"#
    )
}

/// A list signed by a fresh key, checked; with that key.
fn read(json: &str) -> anyhow::Result<(Index, String)> {
    let (key, signature) = signed(json);
    index::verify(json.as_bytes(), &signature, &key, now()).map(|index| (index, key))
}

fn cover(path: &str, size: u64) -> String {
    format!(r#", "cover": {{"path": "{path}", "sha256": "{SHA}", "size": {size}}}"#)
}

const FULL: &str = r#", "kind": "plugin", "category": "chat", "tags": ["bot", "telegram"],
    "stars": 12, "created": "2026-01-02T00:00:00Z", "updated": "2026-03-04T05:06:07+02:00""#;

#[test]
fn formats_1_and_2_are_read() {
    let (index, _) = read(&list(1, &entry("hello", ""), "")).unwrap();
    assert_eq!(index.plugins[0].kind, Kind::Plugin);

    let extra = format!("{FULL}{}", cover("covers/hello.webp", 1000));
    let pack = entry(
        "starter",
        r#", "kind": "pack", "skills": ["review", "tests"]"#,
    );
    let mirror = r#", "verified": [{"name": "friends", "url": "https://friends.example.com/index.json", "key": "ajB547F8pPEYYXGxEYBRZgpgw+iU57ydMrr6VtaLwBQ="}]"#;
    let json = list(2, &format!("{}, {pack}", entry("hello", &extra)), mirror);
    let (index, _) = read(&json).unwrap();
    let hello = &index.plugins[0];
    assert_eq!((hello.category.as_str(), hello.stars), ("chat", 12));
    assert_eq!(hello.cover.as_ref().unwrap().path, "covers/hello.webp");
    assert_eq!(index.plugins[1].skills, ["review", "tests"]);
    assert_eq!(index.verified[0].name, "friends");
}

#[test]
fn an_entry_of_an_unknown_kind_is_left_out_alone() {
    let json = list(
        2,
        &format!(
            r#"{}, {{"name": "x", "kind": "theme", "future": true}}"#,
            entry("hello", "")
        ),
        "",
    );
    let (index, _) = read(&json).unwrap();
    assert_eq!(index.plugins.len(), 1);
    assert_eq!(index.plugins[0].name, "hello");
}

#[test]
fn a_bad_format_2_entry_fails_the_list() {
    let verified = r#", "verified": [{"name": "m", "url": "https://m.example.com/i.json", "key": "ajB547F8pPEYYXGxEYBRZgpgw+iU57ydMrr6VtaLwBQ="}]"#;
    for bad in [
        list(1, &entry("hello", FULL), ""),
        list(1, &entry("hello", ""), verified),
        list(2, &entry("hello", r#", "kind": 3"#), ""),
        list(2, &entry("hello", r#", "category": "Chat!""#), ""),
        list(
            2,
            &entry(
                "hello",
                r#", "tags": ["a","b","c","d","e","f","g","h","i"]"#,
            ),
            "",
        ),
        list(2, &entry("hello", r#", "created": "2026-01-02""#), ""),
        list(2, &entry("hello", r#", "skills": ["x"]"#), ""),
        list(
            2,
            &entry("hello", r#", "kind": "pack", "skills": ["../x"]"#),
            "",
        ),
        list(2, &entry("hello", ""), &verified.replace("https", "http")),
        list(2, &entry("hello", ""), &verified.replace("ajB5", "short")),
    ] {
        assert!(read(&bad).is_err(), "{bad}");
    }
}

#[test]
fn a_cover_stays_a_small_picture_beside_its_list() {
    for (path, size) in [
        ("../x.png", 10),
        ("covers/../../x.png", 10),
        ("/etc/x.png", 10),
        ("covers//x.png", 10),
        ("./x.png", 10),
        ("covers\\\\x.png", 10),
        ("https://evil.example.com/x.png", 10),
        ("covers/x.gif", 10),
        ("covers/x.svg", 10),
        ("covers/x.png", 512 * 1024 + 1),
        ("covers/x.png", 0),
    ] {
        let json = list(2, &entry("hello", &cover(path, size)), "");
        assert!(read(&json).is_err(), "{path} {size}");
    }
    index::check_cover(&Cover {
        path: "covers/x.png".into(),
        sha256: SHA.into(),
        size: 512 * 1024,
    })
    .unwrap();
    let url = fetch::beside("https://a.example.com/shop/index.json", "covers/x.png").unwrap();
    assert_eq!(url, "https://a.example.com/shop/covers/x.png");
    assert!(fetch::beside("https://a.example.com/shop/index.json", "../x.png").is_err());
}

fn mirror(name: &str, key: &str) -> Mirror {
    Mirror {
        name: name.into(),
        url: format!("https://{name}.example.com/index.json"),
        key: key.into(),
    }
}

/// The official list, as the key built in would have checked it, vouching for `verified`.
fn official(verified: &[Mirror]) -> Index {
    let mut index = read(&list(2, &entry("hello", ""), "")).unwrap().0;
    index.verified = verified.to_vec();
    index
}

#[test]
fn a_mirror_never_lists_as_official_and_vouches_for_nobody() {
    // A mirror whose own list names itself, and the official key, as verified.
    let mut own = official(&[]);
    let (_, key) = read(&list(2, &entry("x", ""), "")).unwrap();
    let rogue = mirror("rogue", &key);
    own.verified = vec![rogue.clone(), mirror("other", index::PUBLIC_KEY)];
    let (fetched, failed) = settle(None, vec![(rogue, Ok(own))]);
    assert!(failed.is_empty());
    assert_eq!(fetched[0].tier, Tier::Community);
    assert_eq!(fetched[0].name, "rogue");
}

#[test]
fn a_mirror_is_verified_only_with_the_same_address_and_key() {
    let key_a = read(&list(2, "", "")).unwrap().1;
    let key_b = read(&list(2, "", "")).unwrap().1;
    let vouched = mirror("friends", &key_a);
    let mut moved = vouched.clone();
    moved.url = "https://elsewhere.example.com/index.json".into();
    let mirrors = vec![
        (vouched.clone(), Ok(official(&[]))),
        (mirror("friends", &key_b), Ok(official(&[]))),
        (moved, Ok(official(&[]))),
    ];
    let official = Some(Ok((
        "https://o.example.com/index.json".into(),
        official(&[vouched]),
    )));
    let (fetched, _) = settle(official, mirrors);
    let tiers: Vec<Tier> = fetched.iter().map(|item| item.tier).collect();
    assert_eq!(
        tiers,
        [
            Tier::Official,
            Tier::Verified,
            Tier::Community,
            Tier::Community
        ]
    );
}

#[test]
fn a_failing_source_does_not_block_the_others() {
    let mirrors = vec![
        (mirror("down", "k"), Err(anyhow!("cannot reach it"))),
        (mirror("up", "k"), Ok(official(&[]))),
    ];
    let (fetched, failed) = settle(Some(Err(anyhow!("404"))), mirrors);
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].name, "up");
    let failed: Vec<&str> = failed.iter().map(|item| item.source.as_str()).collect();
    assert_eq!(failed, ["official", "down"]);
}

#[test]
fn a_mirror_must_be_signed_by_its_pinned_key() {
    let json = list(2, &entry("hello", ""), "");
    let (_, signature) = signed(&json);
    let (_, other) = read(&json).unwrap();
    assert!(index::verify(json.as_bytes(), &signature, &other, now()).is_err());
    assert!(index::verify(json.as_bytes(), b"", &other, now()).is_err());

    // In config.yml: http, a reserved name, a short key or a twin are refused. No key
    // makes an Unverified source, whose list `guard_tests` holds to its own rules.
    let keyless: Mirror =
        serde_json::from_str(r#"{"name": "m", "url": "https://m.example.com/i.json"}"#).unwrap();
    assert!(keyless.is_keyless());
    let good = mirror("m", index::PUBLIC_KEY);
    good.check().unwrap();
    for bad in [
        Mirror {
            url: good.url.replace("https", "http"),
            ..good.clone()
        },
        Mirror {
            url: format!("{}?x=1", good.url),
            ..good.clone()
        },
        Mirror {
            name: "official".into(),
            ..good.clone()
        },
        Mirror {
            name: "Local Mirror".into(),
            ..good.clone()
        },
        Mirror {
            key: "c2hvcnQ=".into(),
            ..good.clone()
        },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
    let twins = Workshop {
        sources: vec![good.clone(), good],
    };
    assert!(twins.validate().is_err());
}

#[test]
fn an_update_is_a_greater_semantic_version() {
    for (available, installed, update) in [
        ("1.1.0", "1.0.0", true),
        ("1.0.0", "1.0.0", false),
        ("0.9.0", "1.0.0", false),
        ("1.0.0", "1.0.0-rc.1", true),
        ("1.0.0-rc.2", "1.0.0-rc.1", true),
        ("1.0.0-rc.1", "1.0.0", false),
        ("1.0.0+build.2", "1.0.0+build.1", false),
        ("2", "1.0.0", false),
        ("v2.0.0", "1.0.0", false),
        ("2.0.0", "garbage", false),
        ("", "1.0.0", false),
    ] {
        assert_eq!(
            newer(available, installed).is_some(),
            update,
            "{available} over {installed}"
        );
    }
}

#[test]
fn update_all_leaves_an_update_that_asks_for_more() {
    let folder = crate::testing::temp_dir("store-asks-more").join("hello");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("plugin.yml"),
        "name: hello\nversion: 1.0.0\npermissions:\n  network: [https://a.example.com]\n",
    )
    .unwrap();
    let with = |extra: &str| {
        let json = list(2, &entry("hello", extra), "");
        read(&json).unwrap().0.plugins.remove(0)
    };
    let same = with(r#", "permissions": {"network": ["https://a.example.com"]}"#);
    assert!(!asks_more(&folder, &same).unwrap());
    assert!(!asks_more(&folder, &with("")).unwrap());
    let more =
        with(r#", "permissions": {"network": ["https://a.example.com", "https://b.example.com"]}"#);
    assert!(asks_more(&folder, &more).unwrap());
    let provider = with(r#", "providers": ["https://models.example.com/v1"]"#);
    assert!(asks_more(&folder, &provider).unwrap());
}

#[test]
fn an_old_record_belongs_to_the_official_source() {
    let old = r#"{"serial": 3, "installed": {"hello": {"version": "1.0.0", "sha256": "ab"}}}"#;
    let record: Record = serde_json::from_str(old).unwrap();
    assert_eq!(record.installed["hello"].source, "official");
    assert!(record.mirrors.is_empty());
}

#[test]
fn the_list_comes_in_trust_order() {
    let item = |name: &str, tier: Tier| Listing {
        name: name.into(),
        tier,
        ..Listing::default()
    };
    let mut listings = vec![
        item("b", Tier::Local),
        item("z", Tier::Community),
        item("y", Tier::Official),
        item("a", Tier::Verified),
        item("c", Tier::Official),
    ];
    sort(&mut listings, Order::Trust);
    let names: String = listings.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, "cyazb");
}

/// The official list and a verified mirror carried the store's or a vendor's whole
/// repository's stars, so they never reach a client, whatever the list says; a mirror
/// the user added keeps its count, with the repository it belongs to.
#[test]
fn only_a_mirror_the_user_added_shows_stars_and_with_their_repository() {
    let page = r#", "stars": 900, "homepage": "https://github.com/someone/tool/tree/main/x""#;
    let elsewhere = r#", "stars": 900, "homepage": "https://someone.example.com/tool""#;
    let json = list(
        2,
        &format!("{}, {}", entry("tool", page), entry("other", elsewhere)),
        "",
    );
    let paths = crate::testing::temp_paths("store-stars");
    let record = Record::default();
    let stars = |tier: Tier| -> Vec<Option<RepoStars>> {
        let source = fetched("friends", tier, "k", read(&json).unwrap().0);
        source
            .index
            .plugins
            .iter()
            .map(|entry| {
                listing::listing(&paths, &record, &Default::default(), &source, entry).repo_stars
            })
            .collect()
    };
    assert_eq!(stars(Tier::Official), [None, None]);
    assert_eq!(stars(Tier::Verified), [None, None]);
    let repo = RepoStars {
        repo: "someone/tool".into(),
        stars: 900,
    };
    assert_eq!(stars(Tier::Community), [Some(repo), None]);
}

/// A mirror writes its own count, so the biggest number buys no place on a shelf.
#[test]
fn a_mirror_s_big_star_count_earns_no_shelf() {
    let listings = vec![
        Listing {
            name: "loud".into(),
            tier: Tier::Community,
            repo_stars: Some(RepoStars {
                repo: "someone/loud".into(),
                stars: u32::MAX,
            }),
            ..Listing::default()
        },
        Listing {
            name: "quiet".into(),
            created: Some("2026-01-02T00:00:00Z".into()),
            updated: Some("2026-01-02T00:00:00Z".into()),
            ..Listing::default()
        },
    ];
    let shelves = picks(&listings);
    assert_eq!((shelves.newest, shelves.updated), (vec![1], vec![1]));
}

proptest! {
    /// Whatever a signed list holds, reading it answers and never panics.
    #[test]
    fn any_signed_list_is_read_without_panic(
        name in "\\PC{0,20}",
        version in "\\PC{0,50}",
        kind in "\\PC{0,10}",
        path in "\\PC{0,40}",
        size in any::<u64>(),
        date in "\\PC{0,30}",
    ) {
        let json = serde_json::json!({
            "format": 2, "serial": 1, "expires": "2030-01-01T00:00:00Z",
            "plugins": [{"name": name, "version": version, "kind": kind, "downloads": [],
                "cover": {"path": path, "sha256": SHA, "size": size}, "created": date}],
        })
        .to_string();
        let _ = read(&json);
    }

    #[test]
    fn any_bytes_are_refused_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = read(&text);
    }
}

fn fetched(name: &str, tier: Tier, key: &str, index: Index) -> super::Fetched {
    super::Fetched {
        name: name.into(),
        url: format!("https://{name}.example.com/index.json"),
        key: key.into(),
        tier,
        index,
    }
}

#[test]
fn an_install_needs_the_tier_the_user_saw() {
    let index = read(&list(2, &entry("hello", ""), "")).unwrap().0;
    let sources = [fetched("friends", Tier::Community, "k", index)];
    let mut agreed = super::Agreed {
        source: "friends",
        tier: Tier::Verified,
        name: "hello",
        version: "1.0.0",
        permissions: &[],
        sha256: None,
    };
    let error = super::pick(&sources, &agreed).unwrap_err();
    assert!(error.to_string().contains("Community"), "{error}");
    agreed.tier = Tier::Community;
    assert_eq!(super::pick(&sources, &agreed).unwrap().1.name, "hello");
    agreed.source = "official";
    assert!(super::pick(&sources, &agreed).is_err());
}

#[test]
fn a_name_offered_twice_is_marked() {
    let item = |name: &str, source: &str| Listing {
        name: name.into(),
        source: source.into(),
        ..Listing::default()
    };
    let mut listings = vec![
        item("telegram", "official"),
        item("telegram", "rogue"),
        item("notes", "official"),
    ];
    super::listing::mark_clashes(&mut listings);
    let marked: Vec<bool> = listings.iter().map(|item| item.clash).collect();
    assert_eq!(marked, [true, true, false]);
}

#[test]
fn an_install_belongs_to_the_mirror_address_and_key_that_made_it() {
    let index = || read(&list(2, "", "")).unwrap().0;
    let key_a = read(&list(2, "", "")).unwrap().1;
    let key_b = read(&list(2, "", "")).unwrap().1;
    let source = fetched("friends", Tier::Community, &key_a, index());
    let installed = super::Installed::new(&source, "1.0.0", SHA);
    assert!(installed.is_from(&source));
    // The same name with another key, or at another address, is another mirror.
    assert!(!installed.is_from(&fetched("friends", Tier::Community, &key_b, index())));
    let mut moved = fetched("friends", Tier::Community, &key_a, index());
    moved.url = "https://elsewhere.example.com/index.json".into();
    assert!(!installed.is_from(&moved));
    assert!(!installed.is_from(&fetched("other", Tier::Community, &key_a, index())));

    // An old record is the official list's, by its key, wherever that list now lives.
    let old = r#"{"serial": 3, "installed": {"hello": {"version": "1.0.0", "sha256": "ab"}}}"#;
    let record: Record = serde_json::from_str(old).unwrap();
    let official = fetched("official", Tier::Official, index::PUBLIC_KEY, index());
    assert!(record.installed["hello"].is_from(&official));
    let impostor = fetched("official", Tier::Official, &key_a, index());
    assert!(!record.installed["hello"].is_from(&impostor));
}

#[test]
fn the_store_reaches_only_public_addresses() {
    for private in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "169.254.169.254",
        "0.0.0.0",
        "224.0.0.1",
        "[::1]",
        "[::]",
        "[fc00::1]",
        "[fe80::1]",
        "[ff02::1]",
        "[::ffff:127.0.0.1]",
        "[::ffff:10.0.0.1]",
        "[::ffff:169.254.169.254]",
    ] {
        let url = reqwest::Url::parse(&format!("https://{private}/index.json")).unwrap();
        assert!(fetch::check_url(&url).is_err(), "{private}");
    }
    for public in ["8.8.8.8", "[2606:4700:4700::1111]", "example.com"] {
        let url = reqwest::Url::parse(&format!("https://{public}/index.json")).unwrap();
        fetch::check_url(&url).unwrap();
    }
    let http = reqwest::Url::parse("http://example.com/index.json").unwrap();
    assert!(fetch::check_url(&http).is_err());
}

#[tokio::test]
async fn a_name_that_resolves_here_is_refused() {
    use reqwest::dns::Resolve;
    let name: reqwest::dns::Name = "localhost".parse().unwrap();
    assert!(crate::net::public::PublicOnly.resolve(name).await.is_err());
}

#[test]
fn a_package_follows_only_allowed_redirects() {
    let url = |text: &str| reqwest::Url::parse(text).unwrap();
    let first = url("https://github.com/o/r/releases/download/v1/p.tar.gz");
    let asset = url("https://release-assets.githubusercontent.com/x");
    assert!(fetch::may_follow(&first, &asset, 1));
    assert!(fetch::may_follow(
        &first,
        &url("https://github.com/o/r/x"),
        1
    ));
    assert!(!fetch::may_follow(&first, &asset, 3));
    assert!(!fetch::may_follow(
        &first,
        &url("http://objects.githubusercontent.com/x"),
        1
    ));
    assert!(!fetch::may_follow(
        &first,
        &url("https://evil.example.com/x"),
        1
    ));
    assert!(!fetch::may_follow(&first, &url("https://127.0.0.1/x"), 1));
}

#[test]
fn a_plugin_no_source_installed_is_local() {
    assert_eq!(serde_json::to_string(&Tier::Local).unwrap(), "\"local\"");
    assert!(Tier::Community < Tier::Local);
}

#[test]
fn a_cover_is_known_by_its_first_bytes() {
    use super::picture::sniff;
    assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
    assert_eq!(sniff(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(sniff(b"RIFF\x10\0\0\0WAVEfmt "), None);
    assert_eq!(sniff(b"<svg xmlns="), None);
    assert_eq!(sniff(b"RIFF"), None);
    assert_eq!(sniff(b""), None);
}
