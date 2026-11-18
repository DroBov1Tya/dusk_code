//! Screenshots: what a list may name, and which one a client may fetch.

use dusk_protocol::plugin::Tier;

use super::{
    index::{self, Index},
    picture::{COVER_PIXELS, SCREENSHOT_PIXELS, check_pixels, checked_type, pixels},
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

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes
}

/// A WebP of one chunk `kind` whose data starts with `data`.
fn webp(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
    bytes.extend(kind);
    bytes.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
    bytes.extend(data);
    bytes
}

fn vp8(width: u16, height: u16) -> Vec<u8> {
    let mut data = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
    data.extend(width.to_le_bytes());
    data.extend(height.to_le_bytes());
    webp(b"VP8 ", &data)
}

fn vp8l(width: u32, height: u32) -> Vec<u8> {
    let bits = (width - 1) | ((height - 1) << 14);
    let mut data = vec![0x2f];
    data.extend(bits.to_le_bytes());
    webp(b"VP8L", &data)
}

fn vp8x(flags: u8, width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![flags, 0, 0, 0];
    data.extend(&(width - 1).to_le_bytes()[..3]);
    data.extend(&(height - 1).to_le_bytes()[..3]);
    webp(b"VP8X", &data)
}

#[test]
fn a_picture_declaring_too_many_pixels_is_refused() {
    for (bytes, size) in [
        (png(1200, 630), (1200, 630)),
        (vp8(1200, 630), (1200, 630)),
        (vp8l(1600, 1000), (1600, 1000)),
        (vp8x(0x10, 2400, 1260), (2400, 1260)),
    ] {
        assert_eq!(pixels(&bytes), Some(size));
        assert!(check_pixels(&bytes, COVER_PIXELS).is_ok());
    }
    // Header-only bombs: a few bytes that would decode into gigabytes.
    for bomb in [
        png(60000, 60000),
        png(3201, 10),
        vp8(16383, 16383),
        vp8l(16384, 16384),
        vp8x(0, 60000, 60000),
    ] {
        assert!(check_pixels(&bomb, COVER_PIXELS).is_err());
        assert!(check_pixels(&bomb, SCREENSHOT_PIXELS).is_err());
    }
    assert!(check_pixels(&png(3200, 2000), SCREENSHOT_PIXELS).is_ok());
    assert!(check_pixels(&png(3200, 2000), COVER_PIXELS).is_err());
    assert!(check_pixels(&png(0, 10), COVER_PIXELS).is_err());
}

#[test]
fn an_animated_or_unreadable_picture_is_refused() {
    assert!(check_pixels(&vp8x(0x02, 100, 100), COVER_PIXELS).is_err());
    let unreadable: [&[u8]; 5] = [
        b"\x89PNG\r\n\x1a\nrest",
        b"RIFF\x10\0\0\0WEBPVP8 ",
        b"RIFF\x10\0\0\0WEBPALPH\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        b"<svg xmlns=",
        b"",
    ];
    for bytes in unreadable {
        assert!(pixels(bytes).is_none());
        assert!(check_pixels(bytes, COVER_PIXELS).is_err());
    }
    for cut in 0..vp8(10, 10).len() {
        let _ = pixels(&vp8(10, 10)[..cut]);
    }
}

#[test]
fn the_real_pictures_pass() {
    let cover = include_bytes!("../../../../clients/desktop/src/dev/media/duskcode-agents.webp");
    let shot = include_bytes!("../../../../clients/desktop/src/dev/media/engineering-1.webp");
    assert_eq!(pixels(cover), Some((1200, 630)));
    assert!(check_pixels(cover, COVER_PIXELS).is_ok());
    assert!(check_pixels(shot, SCREENSHOT_PIXELS).is_ok());
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
    source::remember(&[Fetched {
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
