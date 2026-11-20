//! The Contents tab: only listed paths are answered, as escaped text, within the caps.

use std::fs;

use dusk_protocol::plugin::{LOCAL_SOURCE, PREVIEW_BYTES};
use tar::EntryType::{Directory, Regular};

use super::{
    contents::{self, list_package, preview, read_package_file},
    tests::package,
};
use crate::{plugins::catalog, testing::temp_paths};

#[test]
fn a_package_lists_its_files_and_tells_text_from_programs() {
    let big = vec![b'a'; PREVIEW_BYTES + 10];
    let bytes = package(&[
        ("./", Directory, b""),
        ("plugin.yml", Regular, b"name: hello\n"),
        ("bin/", Directory, b""),
        ("bin/run", Regular, b"\x7fELF\0\0\0"),
        ("README.md", Regular, b"# Hello\n"),
        ("big.txt", Regular, &big),
    ]);
    let files = list_package(&bytes).unwrap();
    let shown: Vec<(&str, bool)> = files
        .iter()
        .map(|file| (file.path.as_str(), file.text))
        .collect();
    assert_eq!(
        shown,
        [
            ("README.md", true),
            ("big.txt", true),
            ("bin/run", false),
            ("plugin.yml", true)
        ]
    );
    // A file past the cap comes cut, never whole.
    let read = read_package_file(&bytes, "big.txt").unwrap();
    assert_eq!(read.len(), PREVIEW_BYTES + 1);
    let shown = preview(&read, true).unwrap();
    assert!(shown.truncated && shown.text.len() == PREVIEW_BYTES);
    assert!(read_package_file(&bytes, "../plugin.yml").is_err());
}

#[test]
fn a_package_out_of_the_rules_is_not_listed() {
    let many: Vec<(String, tar::EntryType, &[u8])> = (0..=super::unpack::MAX_FILES)
        .map(|n| (format!("f{n}"), Regular, &b""[..]))
        .collect();
    let many: Vec<(&str, tar::EntryType, &[u8])> = many
        .iter()
        .map(|(name, kind, body)| (name.as_str(), *kind, *body))
        .collect();
    for (why, bytes) in [
        ("parent", package(&[("../out", Regular, b"x")])),
        (
            "twice",
            package(&[("a", Regular, b"x"), ("a", Regular, b"y")]),
        ),
        ("too many files", package(&many)),
    ] {
        assert!(list_package(&bytes).is_err(), "{why}");
    }
}

#[test]
fn preview_escapes_what_could_hide_text() {
    let shown = preview("a\u{202e}b\u{1b}[2J\r\nc\td".as_bytes(), false).unwrap();
    assert_eq!(shown.text, "a\\u{202e}b\\u{1b}[2J\nc\td");
    assert!(preview(b"\xff\xfe", false).is_err());
    // A cut inside a character at the cap is dropped, not refused.
    let mut cut = vec![b'a'; PREVIEW_BYTES - 1];
    cut.extend("é".as_bytes());
    assert_eq!(preview(&cut, true).unwrap().text.len(), PREVIEW_BYTES - 1);
}

#[tokio::test]
async fn only_a_listed_path_of_a_folder_is_answered() {
    let paths = temp_paths("contents-folder");
    let folder = catalog::folder(&paths).join("hello");
    fs::create_dir_all(folder.join("skills/x")).unwrap();
    fs::write(folder.join("plugin.yml"), "name: hello\n").unwrap();
    fs::write(folder.join("skills/x/SKILL.md"), "---\n---\nhi").unwrap();
    let secret = paths.config_root().join("secret.txt");
    fs::write(&secret, "token").unwrap();
    std::os::unix::fs::symlink(&secret, folder.join("link")).unwrap();

    let listed = contents::contents(&paths, &[], LOCAL_SOURCE, "hello")
        .await
        .unwrap();
    assert!(listed.installed);
    let names: Vec<&str> = listed.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        names,
        ["plugin.yml", "skills/x/SKILL.md"],
        "links are left out"
    );

    let text = contents::file(LOCAL_SOURCE, "hello", "skills/x/SKILL.md")
        .await
        .unwrap();
    assert_eq!(text.text, "---\n---\nhi");
    for bad in [
        "link",
        "../secret.txt",
        "skills/../../../secret.txt",
        "/etc/passwd",
    ] {
        assert!(
            contents::file(LOCAL_SOURCE, "hello", bad).await.is_err(),
            "{bad}"
        );
    }
    assert!(
        contents::contents(&paths, &[], LOCAL_SOURCE, "../config")
            .await
            .is_err()
    );
}
