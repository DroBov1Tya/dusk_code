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
    format!(r#"{{"format": 1, "plugins": [{entry}]}}"#)
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
    let index = index::verify(json.as_bytes(), &signature, &key).unwrap();
    assert_eq!(index.plugins[0].name, "hello");

    let other = list(&entry(r#", "description": "changed""#));
    assert!(index::verify(other.as_bytes(), &signature, &key).is_err());
    let (stranger, _) = signed(&json);
    assert!(index::verify(json.as_bytes(), &signature, &stranger).is_err());
    assert!(index::verify(json.as_bytes(), b"not base64!", &key).is_err());
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
    ] {
        let (key, signature) = signed(&bad);
        assert!(
            index::verify(bad.as_bytes(), &signature, &key).is_err(),
            "{bad}"
        );
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
    let index = index::verify(json.as_bytes(), &signature, &key).unwrap();
    let error = place(&root, &index.plugins[0], &bytes, SHA).unwrap_err();
    assert!(format!("{error:#}").contains("permissions"), "{error:#}");
    assert!(!root.join("hello").exists());

    // Without the extra permission it goes in, and a folder the user made is kept.
    let manifest = "name: hello\nversion: 1.0.0\n";
    let bytes = package(&[("plugin.yml", tar::EntryType::Regular, manifest.as_bytes())]);
    place(&root, &index.plugins[0], &bytes, SHA).unwrap();
    assert!(marker(&root.join("hello")).is_some());
    std::fs::remove_file(root.join("hello").join(MARKER)).unwrap();
    assert!(place(&root, &index.plugins[0], &bytes, SHA).is_err());
    assert!(root.join("hello/plugin.yml").is_file());
}
