use std::io::Write;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use base64::Engine;
use flate2::{Compression, write::GzEncoder};

use super::{index, unpack::unpack, *};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn signed(json: &str) -> (String, Vec<u8>) {
    let key = Ed25519KeyPair::generate().unwrap();
    let signature = B64.encode(key.sign(json.as_bytes()).as_ref());
    (
        B64.encode(key.public_key().as_ref()),
        signature.into_bytes(),
    )
}

const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn list(entry: &str) -> String {
    format!(
        r#"{{"format": 1, "serial": 3, "expires": "2030-01-01T00:00:00Z", "plugins": [{entry}]}}"#
    )
}

fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-11-14T00:00:00Z".parse().unwrap()
}

fn verify(json: &str, signature: &[u8], key: &str) -> Result<index::Index> {
    index::verify(json.as_bytes(), signature, key, now())
}

fn entry(extra: &str) -> String {
    format!(
        r#"{{"name": "hello", "version": "1.0.0", "downloads": [{{"url": "https://example.com/h.tar.gz", "sha256": "{SHA}", "size": 10}}]{extra}}}"#
    )
}

#[test]
fn a_list_is_read_only_with_its_signature() {
    let json = list(&entry(""));
    let (key, signature) = signed(&json);
    let index = verify(&json, &signature, &key).unwrap();
    assert_eq!(index.plugins[0].name, "hello");

    let other = list(&entry(r#", "description": "changed""#));
    assert!(verify(&other, &signature, &key).is_err());
    let (stranger, _) = signed(&json);
    assert!(verify(&json, &signature, &stranger).is_err());
    assert!(verify(&json, b"not base64!", &key).is_err());
}

#[test]
fn a_signed_list_is_still_checked() {
    for bad in [
        list(&entry(r#", "homepage": "http://example.com""#)),
        list(&entry("").replace("https://example.com", "http://example.com")),
        list(&entry("").replace(SHA, "abc")),
        list(&entry("").replace("\"size\": 10", "\"size\": 0")),
        list(&entry("").replace("\"hello\"", "\"../x\"")),
        list(&entry(r#", "secret": 1"#)),
        list(&entry("")).replace("\"format\": 1", "\"format\": 2"),
        list(&entry("")).replace("2030-01-01", "2026-01-01"),
        list(&entry("")).replace("2030-01-01T00:00:00Z", "soon"),
        list(&format!("{}, {}", entry(""), entry(""))),
        list(&entry(r#", "providers": ["ftp://example.com"]"#)),
    ] {
        let (key, signature) = signed(&bad);
        assert!(verify(&bad, &signature, &key).is_err(), "{bad}");
    }
}

/// A tar.gz with entries written byte by byte, so it can hold what a builder refuses.
fn package(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, kind, body) in entries {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(*kind);
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        if *kind == tar::EntryType::Symlink || *kind == tar::EntryType::Link {
            header.set_link_name("/etc/passwd").unwrap();
        }
        header.set_cksum();
        tar.extend_from_slice(header.as_bytes());
        tar.extend_from_slice(body);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.extend_from_slice(&[0; 1024]);
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

#[test]
fn a_package_unpacks_only_into_its_folder() {
    use tar::EntryType::{Directory, Link, Regular, Symlink};
    let root = crate::testing::temp_dir("store-unpack");
    std::fs::create_dir_all(&root).unwrap();
    let good = package(&[
        ("./", Directory, b""),
        ("./plugin.yml", Regular, b"name: hello\n"),
        ("bin/", Directory, b""),
        ("bin/run", Regular, b"#!/bin/sh\n"),
    ]);
    unpack(&good, &root.join("good")).unwrap();
    assert!(root.join("good/bin/run").is_file());

    for (why, bad) in [
        ("parent", package(&[("../out", Regular, b"x")])),
        ("deep parent", package(&[("a/../../out", Regular, b"x")])),
        ("absolute", package(&[("/tmp/out", Regular, b"x")])),
        ("symlink", package(&[("link", Symlink, b"")])),
        ("hard link", package(&[("link", Link, b"")])),
        (
            "same file twice",
            package(&[("a", Regular, b"x"), ("a", Regular, b"y")]),
        ),
        ("not a tar.gz", b"plain text".to_vec()),
        ("file without a name", package(&[("./", Regular, b"x")])),
    ] {
        let into = root.join(why.replace(' ', "-"));
        assert!(unpack(&bad, &into).is_err(), "{why}");
    }
    assert!(!root.join("out").exists());
    assert!(!Path::new("/tmp/out").exists());
}

#[test]
fn a_package_asking_more_than_its_listing_is_refused() {
    let root = crate::testing::temp_dir("store-place");
    std::fs::create_dir_all(&root).unwrap();
    let manifest =
        "name: hello\nversion: 1.0.0\npermissions:\n  network: [https://evil.example.com]\n";
    let bytes = package(&[("plugin.yml", tar::EntryType::Regular, manifest.as_bytes())]);
    let json = list(&entry(""));
    let (key, signature) = signed(&json);
    let index = verify(&json, &signature, &key).unwrap();
    let error = place(&root, &index.plugins[0], &bytes, false).unwrap_err();
    assert!(format!("{error:#}").contains("permissions"), "{error:#}");
    assert!(!root.join("hello").exists());

    // A provider the listing does not show is refused too.
    let manifest = "name: hello\nversion: 1.0.0\nproviders:\n  x: {kind: openai_compatible, base_url: https://x.example.com/v1}\n";
    let bytes = package(&[("plugin.yml", tar::EntryType::Regular, manifest.as_bytes())]);
    let error = place(&root, &index.plugins[0], &bytes, false).unwrap_err();
    assert!(format!("{error:#}").contains("providers"), "{error:#}");

    // As listed it goes in; a folder the store does not own is never replaced, one it
    // owns is.
    let manifest = "name: hello\nversion: 1.0.0\n";
    let bytes = package(&[("plugin.yml", tar::EntryType::Regular, manifest.as_bytes())]);
    place(&root, &index.plugins[0], &bytes, false).unwrap();
    assert!(root.join("hello/plugin.yml").is_file());
    assert!(place(&root, &index.plugins[0], &bytes, false).is_err());
    assert!(root.join("hello/plugin.yml").is_file());
    place(&root, &index.plugins[0], &bytes, true).unwrap();
    assert!(!root.join(STAGING).exists());
}

/// The live store, end to end: the signed list on GitHub Pages, a package from a release.
#[tokio::test]
#[ignore = "reaches the network"]
async fn the_live_store_installs_a_plugin() {
    let paths = crate::testing::temp_paths("store-live");
    crate::init::create_private_dir(&super::super::catalog::folder(&paths)).unwrap();
    let listed = super::list(&paths).await.unwrap();
    let telegram = listed.iter().find(|item| item.name == "telegram").unwrap();
    install(&paths, "telegram", &telegram.version, &telegram.permissions)
        .await
        .unwrap();
    let changed = ["connect to https://evil.example.com".to_owned()];
    assert!(
        install(&paths, "telegram", &telegram.version, &changed)
            .await
            .is_err()
    );
    let folder = super::super::catalog::folder(&paths).join("telegram");
    assert!(folder.join("bin/telegram").is_file());
    let listed = super::list(&paths).await.unwrap();
    let again = listed.iter().find(|item| item.name == "telegram").unwrap();
    assert_eq!(again.installed.as_deref(), Some(telegram.version.as_str()));
    remove(&paths, "telegram").await.unwrap();
    assert!(!folder.exists());
}
