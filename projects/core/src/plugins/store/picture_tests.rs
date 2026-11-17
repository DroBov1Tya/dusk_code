//! Screenshots: what a list may name, and which one a client may fetch.

use dusk_protocol::plugin::Tier;

use super::{
    index::{self, Index},
    picture::checked_type,
    source::{self, Fetched},
    tests::{SHA, signed},
};
use crate::config::Mirror;

fn list(entries: &str) -> String {
    format!(
        r#"{{"format": 2, "serial": 3, "expires": "2030-01-01T00:00:00Z", "plugins": [{entries}]}}"#
    )
}

fn entry(name: &str, screenshots: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "downloads": [{{"url": "https://example.com/h.tar.gz", "sha256": "{SHA}", "size": 10}}], "screenshots": [{screenshots}]}}"#
    )
}

fn shot(path: &str, size: u64, caption: Option<&str>) -> String {
    let caption = caption.map_or(String::new(), |text| {
        format!(r#", "caption": {}"#, serde_json::to_string(text).unwrap())
    });
    format!(r#"{{"path": "{path}", "sha256": "{SHA}", "size": {size}{caption}}}"#)
}

fn read(json: &str) -> anyhow::Result<(Index, String)> {
    let (key, signature) = signed(json);
    let now = "2026-11-14T00:00:00Z".parse().unwrap();
    index::verify(json.as_bytes(), &signature, &key, now).map(|index| (index, key))
}

#[test]
fn a_list_names_screenshots_with_captions() {
    let shots = [
        shot("screenshots/hello/1.png", 1000, Some("The list of skills")),
        shot("screenshots/hello/2.webp", 1024 * 1024, None),
    ]
    .join(", ");
    let (index, _) = read(&list(&entry("hello", &shots))).unwrap();
    let hello = &index.plugins[0];
    assert_eq!(hello.screenshots.len(), 2);
    assert_eq!(
        hello.screenshots[0].caption.as_deref(),
        Some("The list of skills")
    );
    assert_eq!(hello.screenshots[1].path, "screenshots/hello/2.webp");
}

#[test]
fn more_than_eight_screenshots_refuse_the_list() {
    let shots: Vec<String> = (0..9)
        .map(|n| shot(&format!("s/{n}.png"), 10, None))
        .collect();
    assert!(read(&list(&entry("hello", &shots[..8].join(", ")))).is_ok());
    assert!(read(&list(&entry("hello", &shots.join(", ")))).is_err());
}

#[test]
fn a_bad_screenshot_refuses_the_list() {
    let long = "a".repeat(index::MAX_CAPTION + 1);
    let bad = [
        shot("../x.png", 10, None),
        shot("s/../../x.png", 10, None),
        shot("/s/x.png", 10, None),
        shot("https://evil.example.com/x.png", 10, None),
        shot("s//x.png", 10, None),
        shot("s/x.gif", 10, None),
        shot("s/x.svg", 10, None),
        shot("s/x.png", 0, None),
        shot("s/x.png", index::MAX_SCREENSHOT_BYTES + 1, None),
        shot("s/x.png", 10, Some(&long)),
        shot("s/x.png", 10, Some("")),
        shot("s/x.png", 10, Some(" padded")),
        shot("s/x.png", 10, Some("two\nlines")),
        shot("s/x.png", 10, Some("bell\u{7}")),
        shot("s/x.png", 10, Some("flip\u{202e}gnp.exe")),
        r#"{"path": "s/x.png", "sha256": "abc", "size": 10}"#.to_owned(),
        format!(r#"{{"path": "s/x.png", "sha256": "{SHA}", "size": 10, "url": "x"}}"#),
    ];
    for screenshot in bad {
        let json = list(&entry("hello", &screenshot));
        assert!(read(&json).is_err(), "{screenshot}");
    }
    let exact = "é".repeat(index::MAX_CAPTION);
    assert!(read(&list(&entry("hello", &shot("s/x.png", 10, Some(&exact))))).is_ok());
}

#[test]
fn a_format_1_list_has_no_screenshots() {
    let json = list(&entry("hello", &shot("s/x.png", 10, None)))
        .replace(r#""format": 2"#, r#""format": 1"#);
    assert!(read(&json).is_err());
}

#[test]
fn a_screenshot_must_be_the_picture_its_name_says() {
    let png = b"\x89PNG\r\n\x1a\nrest";
    let webp = b"RIFF\x10\0\0\0WEBPVP8 ";
    assert_eq!(checked_type("s/x.png", png).unwrap(), "image/png");
    assert_eq!(checked_type("s/x.webp", webp).unwrap(), "image/webp");
    assert!(checked_type("s/x.png", webp).is_err());
    assert!(checked_type("s/x.webp", png).is_err());
    assert!(checked_type("s/x.png", b"<svg xmlns=").is_err());
}

fn mirror(name: &str, key: &str) -> Mirror {
    Mirror {
        name: name.into(),
        url: format!("https://{name}.example.com/index.json"),
        key: key.into(),
    }
}

/// Remembers a mirror `name`'s list with two screenshots of `hello`; with its mirror.
fn remembered(name: &str) -> Mirror {
    let shots = [shot("s/1.png", 10, None), shot("s/2.png", 10, None)].join(", ");
    let (index, key) = read(&list(&entry("hello", &shots))).unwrap();
    let mirror = mirror(name, &key);
    source::remember_pictures(&[Fetched {
        name: mirror.name.clone(),
        url: mirror.url.clone(),
        key: key.clone(),
        tier: Tier::Community,
        index,
    }]);
    mirror
}

#[test]
fn only_a_screenshot_the_list_named_is_fetched() {
    let mirror = remembered("shots-index");
    let mirrors = [mirror];
    let seen = source::seen(&mirrors, "shots-index", "hello").unwrap();
    assert_eq!(source::screenshot(&seen, 1).unwrap().path, "s/2.png");
    assert!(source::screenshot(&seen, 2).is_none());
    assert!(source::screenshot(&seen, u32::MAX).is_none());
    assert!(source::seen(&mirrors, "shots-index", "other").is_err());
    assert!(source::seen(&mirrors, "nobody", "hello").is_err());
}

#[test]
fn a_mirror_screenshot_needs_the_same_address_and_key() {
    let mirror = remembered("shots-mirror");
    assert!(source::seen(std::slice::from_ref(&mirror), "shots-mirror", "hello").is_ok());

    let moved = Mirror {
        url: "https://elsewhere.example.com/index.json".into(),
        ..mirror.clone()
    };
    assert!(source::seen(&[moved], "shots-mirror", "hello").is_err());

    let (other_key, _) = signed("{}");
    let rekeyed = Mirror {
        key: other_key,
        ..mirror.clone()
    };
    assert!(source::seen(&[rekeyed], "shots-mirror", "hello").is_err());
    assert!(source::seen(&[], "shots-mirror", "hello").is_err());
}
