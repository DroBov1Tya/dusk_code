//! Release connectors: a program from one exact GitHub release, held to the official
//! list's own work, downloaded only from GitHub, checked by SHA-256 before it is
//! unpacked, and unpacked under the rules that keep it inside its folder.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use dusk_protocol::{plugin::Tier, text::sha256_hex};
use tar::EntryType::{Directory, Link, Regular, Symlink};

use super::{
    connector::{
        self, install,
        release::{self, Asset, Release},
    },
    fetch,
    index::{self, Badge},
    listing,
    source::settle,
    tests::package,
};
use crate::config::Mirror;

const SHA: &str = "a4ce78ecddb88b91ba9c8bb4df42fb492295a07bab3ff6e2a47a156bf326d7ee";
const PROGRAM: &str = "pentacore-0.8.0/pentacore";

fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-11-19T00:00:00Z".parse().unwrap()
}

/// An entry of the official list with `server`, made by `author` under `badge`.
fn entry(server: &str, author: &str, badge: &str) -> String {
    format!(
        r#"{{"name": "pentacore", "version": "1.0.0", "kind": "release-connector", "description": "Memory.", "server": {server}, "author": "{author}", "license": "MIT", "source": "https://github.com/DroBov1Tya/PentaCore", "categories": ["productivity"], "badge": "{badge}"}}"#
    )
}

fn release(assets: &str) -> String {
    format!(
        r#"{{"release": {{"repository": "DroBov1Tya/PentaCore", "tag": "v0.8.0", "assets": {assets}}}, "run_by": "vendor", "reaches": "a memory kept on this computer"}}"#
    )
}

fn asset(target: &str, file: &str) -> String {
    format!(r#""{target}": {{"file": "{file}", "sha256": "{SHA}", "binary": "{PROGRAM}"}}"#)
}

fn list(entries: &[String]) -> String {
    format!(
        r#"{{"format": 3, "serial": 1, "expires": "2030-01-01T00:00:00Z", "plugins": [{}]}}"#,
        entries.join(",")
    )
}

fn parse(json: &str) -> anyhow::Result<index::Index> {
    index::parse(json.as_bytes(), now())
}

/// A release with an archive for every triple this computer runs.
fn here_release(size: Option<u64>, sha256: &str) -> Release {
    let assets = release::here()
        .iter()
        .map(|target| {
            let file = format!("pentacore-0.8.0-{target}.tar.gz");
            let asset = Asset {
                file,
                sha256: sha256.to_owned(),
                binary: PROGRAM.to_owned(),
                size,
            };
            ((*target).to_owned(), asset)
        })
        .collect();
    Release {
        repository: "DroBov1Tya/PentaCore".into(),
        tag: "v0.8.0".into(),
        assets,
    }
}

#[test]
fn a_release_is_read_only_pinned_and_plain() {
    let darwin = asset(
        "aarch64-apple-darwin",
        "pentacore-0.8.0-aarch64-apple-darwin.tar.gz",
    );
    let windows = asset(
        "x86_64-pc-windows-msvc",
        "pentacore-0.8.0-x86_64-pc-windows-msvc.zip",
    );
    let good = release(&format!("{{{darwin}, {windows}}}"));
    let index = parse(&list(&[entry(&good, "DroBov1Tya", "official")])).unwrap();
    let server = index.plugins[0].server.as_ref().unwrap();
    assert_eq!(server.release.as_ref().unwrap().tag, "v0.8.0");
    assert_eq!(
        index.plugins[0].kind,
        dusk_protocol::plugin::Kind::Connector
    );

    // The two kinds cannot drift: a plain connector never runs a release, and a release
    // connector always does.
    let plain = entry(&good, "DroBov1Tya", "official")
        .replace(r#""kind": "release-connector""#, r#""kind": "connector""#);
    assert!(parse(&list(&[plain])).is_err());
    let remote = r#"{"url": "https://mcp.example.com/mcp", "run_by": "vendor", "reaches": "x"}"#;
    assert!(parse(&list(&[entry(remote, "DroBov1Tya", "official")])).is_err());

    let bad = [
        ("latest", good.replace("v0.8.0", "latest")),
        ("tag as a path", good.replace("v0.8.0", "../v0.8.0")),
        (
            "another host",
            good.replace("DroBov1Tya/PentaCore", "evil.example/x/y"),
        ),
        (
            "repository as a path",
            good.replace("DroBov1Tya/PentaCore", "../PentaCore"),
        ),
        (
            "file as a path",
            good.replace("pentacore-0.8.0-aarch64", "../a"),
        ),
        (
            "zip for a unix",
            good.replace("aarch64-apple-darwin.tar.gz", "aarch64-apple-darwin.zip"),
        ),
        ("short hash", good.replace(SHA, &SHA[..63])),
        ("upper-case hash", good.replace(SHA, &SHA.to_uppercase())),
        (
            "unknown platform",
            good.replace(r#""aarch64-apple-darwin":"#, r#""sparc-sun-solaris":"#),
        ),
        ("program out", good.replace(PROGRAM, "../pentacore")),
        ("program absolute", good.replace(PROGRAM, "/bin/sh")),
        ("no archives", release("{}")),
        (
            "too large",
            good.replace(
                r#""binary""#,
                &format!(r#""size": {}, "binary""#, release::MAX_DOWNLOAD + 1),
            ),
        ),
        (
            "a command too",
            good.replace(r#""run_by""#, r#""command": "npx", "run_by""#),
        ),
        (
            "unknown field",
            good.replace(r#""tag""#, r#""latest": true, "tag""#),
        ),
    ];
    for (why, server) in bad {
        let json = list(&[entry(&server, "DroBov1Tya", "official")]);
        assert!(parse(&json).is_err(), "{why}");
    }
}

#[test]
fn only_the_official_lists_own_work_brings_a_release() {
    let darwin = asset(
        "aarch64-apple-darwin",
        "pentacore-0.8.0-aarch64-apple-darwin.tar.gz",
    );
    let server = release(&format!("{{{darwin}}}"));
    let ours = entry(&server, "DroBov1Tya", "official");
    let theirs = entry(&server, "Somebody", "verified");
    let index = parse(&list(std::slice::from_ref(&ours))).unwrap();
    let spec = index.plugins[0].server.clone().unwrap();
    let refused = |tier, badge| {
        spec.unfit("pentacore", tier, badge)
            .is_some_and(|why| why.contains("only the official list"))
    };
    assert!(!refused(Tier::Official, Some(Badge::Official)));
    for tier in [
        Tier::Verified,
        Tier::Community,
        Tier::Unverified,
        Tier::Local,
    ] {
        assert!(refused(tier, Some(Badge::Official)), "{tier:?}");
    }
    assert!(refused(Tier::Official, Some(Badge::Verified)));
    assert!(refused(Tier::Official, None));

    // Somebody else's work in the official list earns Verified, and is refused.
    let fetched = settle(
        Some(Ok((
            "https://o.example.com/index.json".into(),
            parse(&list(&[theirs])).unwrap(),
        ))),
        Vec::new(),
    )
    .0;
    let entry = &fetched[0].index.plugins[0];
    let tier = listing::tier(&fetched[0], entry);
    assert_eq!(tier, Tier::Verified);
    assert!(
        entry
            .server
            .as_ref()
            .unwrap()
            .unfit("pentacore", tier, entry.badge)
            .is_some()
    );

    // A list nobody signed keeps no program.
    let mirror = Mirror {
        name: "mirror".into(),
        url: "https://m.example.com/index.json".into(),
        key: String::new(),
    };
    let (fetched, _) = settle(None, vec![(mirror, parse(&list(&[ours])))]);
    assert!(fetched[0].index.plugins.is_empty());
}

fn entry_json_again() -> String {
    let darwin = asset(
        "aarch64-apple-darwin",
        "pentacore-0.8.0-aarch64-apple-darwin.tar.gz",
    );
    entry(&release(&format!("{{{darwin}}}")), "DroBov1Tya", "official")
}

#[test]
fn a_computer_without_a_build_is_refused() {
    let mut release = here_release(None, SHA);
    assert!(release.unfit("pentacore").is_none() || release::here().is_empty());
    // An archive for a platform this computer is not.
    let elsewhere = match cfg!(target_os = "macos") {
        true => "x86_64-unknown-linux-gnu",
        false => "aarch64-apple-darwin",
    };
    let one = release.assets.values().next().cloned().unwrap();
    release.assets = [(elsewhere.to_owned(), one)].into();
    let why = release.unfit("pentacore").unwrap();
    assert!(why.contains("no build for this computer"), "{why}");
    assert!(release.asset().is_none());
    assert!(release.consent("pentacore").contains("no build"));
    let paths = crate::testing::temp_paths("release-none");
    assert!(release.binary_path(&paths, "pentacore").is_none());
    assert_eq!(
        release.asset_for(&["sparc", elsewhere]).map(|a| &a.file),
        release.assets.get(elsewhere).map(|a| &a.file)
    );
}

#[test]
fn the_archive_comes_only_from_the_release_on_github() {
    let release = here_release(Some(10), SHA);
    let Some(asset) = release.asset() else {
        return;
    };
    let url = release.url(asset);
    assert_eq!(
        url,
        format!(
            "https://github.com/DroBov1Tya/PentaCore/releases/download/v0.8.0/{}",
            asset.file
        )
    );
    let first = reqwest::Url::parse(&url).unwrap();
    let next = |to: &str| fetch::may_follow(&first, &reqwest::Url::parse(to).unwrap(), 0);
    assert!(next(
        "https://release-assets.githubusercontent.com/github-production-release-asset/1"
    ));
    assert!(next("https://objects.githubusercontent.com/x"));
    for to in [
        "https://evil.example.com/pentacore.tar.gz",
        "http://release-assets.githubusercontent.com/x",
        "https://127.0.0.1/x",
        "https://github.com.evil.example/x",
    ] {
        assert!(!next(to), "{to}");
    }
    // Three hops at most.
    assert!(!fetch::may_follow(
        &first,
        &reqwest::Url::parse("https://objects.githubusercontent.com/x").unwrap(),
        3
    ));
    // The consent names what is fetched and its hash.
    let line = release.consent("pentacore");
    for part in [
        "github.com/DroBov1Tya/PentaCore",
        "v0.8.0",
        &asset.file,
        &SHA[..12],
        "your rights",
    ] {
        assert!(line.contains(part), "{part}: {line}");
    }
}

#[test]
fn an_archive_is_checked_before_anything_is_unpacked() {
    let archive = package(&[
        ("pentacore-0.8.0/", Directory, b""),
        (PROGRAM, Regular, b"run"),
    ]);
    let paths = crate::testing::temp_paths("release-hash");
    for (why, release) in [
        ("another hash", here_release(None, &"0".repeat(64))),
        (
            "another size",
            here_release(Some(archive.len() as u64 + 1), &sha256_hex(&archive)),
        ),
    ] {
        let Some(asset) = release.asset() else {
            return;
        };
        let error = install::place(&paths, "pentacore", &release, asset, &archive).unwrap_err();
        assert!(!format!("{error:#}").is_empty(), "{why}");
        // Refused before the folder was even made.
        assert!(!paths.connectors_root().exists(), "{why}");
    }
}

#[test]
fn an_archive_with_links_paths_out_or_too_much_is_refused() {
    let paths = crate::testing::temp_paths("release-unpack");
    let mut huge = tar::Header::new_old();
    huge.as_old_mut().name[..PROGRAM.len()].copy_from_slice(PROGRAM.as_bytes());
    huge.set_entry_type(Regular);
    huge.set_size(super::unpack::MAX_UNPACKED_BYTES + 1);
    huge.set_mode(0o755);
    huge.set_cksum();
    let mut tar = huge.as_bytes().to_vec();
    tar.extend_from_slice(&[0; 1024]);
    let huge = {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    };
    let cases = [
        (
            "parent",
            package(&[(PROGRAM, Regular, b"run"), ("../out", Regular, b"x")]),
        ),
        (
            "absolute",
            package(&[("/tmp/dusk-release-out", Regular, b"x")]),
        ),
        ("symlink program", package(&[(PROGRAM, Symlink, b"")])),
        ("hard link program", package(&[(PROGRAM, Link, b"")])),
        (
            "symlink beside",
            package(&[(PROGRAM, Regular, b"run"), ("lib", Symlink, b"")]),
        ),
        (
            "program twice",
            package(&[(PROGRAM, Regular, b"a"), (PROGRAM, Regular, b"b")]),
        ),
        (
            "program a folder",
            package(&[("pentacore-0.8.0/pentacore/", Directory, b"")]),
        ),
        (
            "no program",
            package(&[("pentacore-0.8.0/README.md", Regular, b"x")]),
        ),
        ("over the cap", huge),
        ("not a tar.gz", b"PK\x03\x04 a zip".to_vec()),
    ];
    for (why, archive) in cases {
        let release = here_release(Some(archive.len() as u64), &sha256_hex(&archive));
        let Some(asset) = release.asset() else {
            return;
        };
        assert!(
            install::place(&paths, "pentacore", &release, asset, &archive).is_err(),
            "{why}"
        );
        let folder = release::folder(&paths, "pentacore");
        assert!(!folder.join("v0.8.0").exists(), "{why}");
        assert!(!folder.join(".staging").exists(), "{why}");
    }
    assert!(!paths.connectors_root().join("out").exists());
    assert!(!Path::new("/tmp/dusk-release-out").exists());
}

#[test]
fn the_program_goes_into_its_tag_folder_and_an_update_leaves_no_old_one() {
    let archive = package(&[
        ("pentacore-0.8.0/", Directory, b""),
        ("pentacore-0.8.0/README.md", Regular, b"read me"),
        (PROGRAM, Regular, b"#!/bin/sh\n"),
    ]);
    let paths = crate::testing::temp_paths("release-place");
    let release = here_release(Some(archive.len() as u64), &sha256_hex(&archive));
    let Some(asset) = release.asset() else {
        return;
    };
    let program = install::place(&paths, "pentacore", &release, asset, &archive).unwrap();
    assert_eq!(
        Some(&program),
        release.binary_path(&paths, "pentacore").as_ref()
    );
    assert_eq!(fs::read(&program).unwrap(), b"#!/bin/sh\n");
    let mode = fs::metadata(&program).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    // Only the program is taken out.
    let tag = release::folder(&paths, "pentacore").join("v0.8.0");
    assert_eq!(fs::read_dir(&tag).unwrap().count(), 1);
    assert_eq!(
        release::tag_of(&paths, "pentacore", &program).as_deref(),
        Some("v0.8.0")
    );
    // The same tag again replaces it, and nothing of the swap is left beside it.
    install::place(&paths, "pentacore", &release, asset, &archive).unwrap();
    assert!(program.is_file());
    let left: Vec<_> = fs::read_dir(release::folder(&paths, "pentacore"))
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    assert_eq!(left, ["v0.8.0"]);
    // An archive refused on the same tag leaves the program in place.
    let mut wrong = asset.clone();
    wrong.sha256 = SHA.into();
    let mut other = release.clone();
    other
        .assets
        .values_mut()
        .for_each(|a| a.sha256 = SHA.into());
    assert!(install::place(&paths, "pentacore", &other, &wrong, &archive).is_err());
    assert!(program.is_file());

    // The config starts it by that path.
    let index = parse(&list(&[entry_json_again()])).unwrap();
    let mut spec = index.plugins[0].server.clone().unwrap();
    spec.release = Some(release.clone());
    let config = spec.config(&paths, "pentacore");
    let target: Vec<String> = serde_json::from_str(&connector::target(&config)).unwrap();
    assert_eq!(target[0], program.to_string_lossy());
    // Another version of the same program keeps what the user filled in; a program
    // elsewhere or other arguments do not.
    let mut older = spec.clone();
    if let Some(release) = &mut older.release {
        release.tag = "v0.7.1".into();
    }
    let older = older.config(&paths, "pentacore");
    assert!(connector::same_program(
        &paths,
        "pentacore",
        &config,
        &older
    ));
    let mut moved = spec.clone();
    moved.args = vec!["--other".into()];
    let moved = moved.config(&paths, "pentacore");
    assert!(!connector::same_program(
        &paths,
        "pentacore",
        &moved,
        &older
    ));
    assert!(!connector::same_program(&paths, "other", &config, &older));

    let old = release::folder(&paths, "pentacore").join("v0.7.1");
    fs::create_dir_all(&old).unwrap();
    install::tidy(&paths, "pentacore", &["v0.8.0".into()]).unwrap();
    assert!(!old.exists() && program.is_file());
    // A version a server still starts, though the record forgot it, is kept.
    fs::create_dir_all(&old).unwrap();
    install::tidy(&paths, "pentacore", &["v0.8.0".into(), "v0.7.1".into()]).unwrap();
    assert!(old.exists() && program.is_file());
    assert_eq!(
        release::owner_of(&paths, &program).as_deref(),
        Some("pentacore")
    );
    assert_eq!(release::owner_of(&paths, Path::new("/usr/bin/env")), None);
    install::tidy(&paths, "pentacore", &[]).unwrap();
    assert!(!release::folder(&paths, "pentacore").exists());
}

/// macOS: the quarantine mark comes off the program, and a program without one is fine.
#[cfg(target_os = "macos")]
#[test]
fn the_quarantine_mark_comes_off_the_program() {
    let root = crate::testing::temp_dir("release-quarantine");
    fs::create_dir_all(&root).unwrap();
    let program = root.join("pentacore");
    fs::write(&program, b"x").unwrap();
    install::unquarantine(&program).unwrap();
    let set = std::process::Command::new("/usr/bin/xattr")
        .args(["-w", "com.apple.quarantine", "0081;00000000;duskcode;"])
        .arg(&program)
        .status()
        .unwrap();
    assert!(set.success());
    install::unquarantine(&program).unwrap();
    let left = std::process::Command::new("/usr/bin/xattr")
        .arg(&program)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&left.stdout).contains("quarantine"));
}

/// PentaCore v0.8.0 from its GitHub release, end to end on an Apple silicon Mac: the
/// archive is downloaded and checked, the program put in place and started as the
/// config starts it, and it answers the MCP `initialize`.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
#[ignore = "reaches the network"]
async fn the_live_release_installs_and_answers() {
    use std::io::{BufRead, BufReader, Write};
    let paths = crate::testing::temp_paths("release-live");
    let release = Release {
        repository: "DroBov1Tya/PentaCore".into(),
        tag: "v0.8.0".into(),
        assets: [(
            "aarch64-apple-darwin".to_owned(),
            Asset {
                file: "pentacore-0.8.0-aarch64-apple-darwin.tar.gz".into(),
                sha256: SHA.into(),
                binary: "pentacore-0.8.0-aarch64-apple-darwin/pentacore".into(),
                size: Some(15_837_154),
            },
        )]
        .into(),
    };
    release.check().unwrap();
    let program = install::install(&paths, "pentacore", &release)
        .await
        .unwrap();
    assert_eq!(
        Some(&program),
        release.binary_path(&paths, "pentacore").as_ref()
    );
    let home = crate::testing::temp_dir("release-live-home");
    let mut child = std::process::Command::new(&program)
        .env("PENTACORE_HOME", &home)
        .env("PENTACORE_PROJECT", "dusk-test")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let hello = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"duskcode-test","version":"0"}}}"#;
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{hello}").unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let answer: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(answer["id"], 1, "{line}");
    assert!(answer["result"]["serverInfo"].is_object(), "{line}");
    eprintln!("{line}");
    install::tidy(&paths, "pentacore", &[]).unwrap();
    assert!(!release::folder(&paths, "pentacore").exists());
    let _ = fs::remove_dir_all(&home);
}
