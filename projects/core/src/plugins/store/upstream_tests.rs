//! Files from upstream: exactly the pinned ones, or nothing.

use super::upstream::{Upstream, UpstreamFile, assemble};

fn pinned(files: &[(&str, &[u8])]) -> Upstream {
    Upstream {
        repo: "someone/skills".into(),
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        files: files
            .iter()
            .map(|(path, body)| UpstreamFile {
                path: format!("skills/x/{path}"),
                to: format!("x/{path}"),
                sha256: dusk_protocol::text::sha256_hex(body),
                size: body.len() as u64,
            })
            .collect(),
    }
}

fn got(files: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    files
        .iter()
        .map(|(path, body)| (format!("skills/x/{path}"), body.to_vec()))
        .collect()
}

const SKILL: (&str, &[u8]) = ("SKILL.md", b"---\nname: x\n---\nDo x.");
const LICENSE: (&str, &[u8]) = ("LICENSE", b"MIT");

#[test]
fn the_pinned_files_are_placed_by_their_target() {
    let upstream = pinned(&[SKILL, LICENSE]);
    upstream.check().unwrap();
    let files = assemble(&upstream, got(&[SKILL, LICENSE])).unwrap();
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        ["x/LICENSE", "x/SKILL.md"]
    );
}

#[test]
fn a_changed_extra_or_missing_file_refuses_all() {
    let upstream = pinned(&[SKILL, LICENSE]);
    let changed = ("SKILL.md", b"---\nname: x\n---\nDo y.".as_slice());
    assert!(assemble(&upstream, got(&[changed, LICENSE])).is_err());
    assert!(assemble(&upstream, got(&[SKILL, LICENSE, ("run.sh", b"rm")])).is_err());
    assert!(assemble(&upstream, got(&[SKILL])).is_err());
    let binary = ("SKILL.md", b"\0\x01".as_slice());
    assert!(assemble(&pinned(&[binary]), got(&[binary])).is_err());
}

#[test]
fn paths_and_pins_are_checked() {
    let good = pinned(&[SKILL]);
    let mut bad = good.clone();
    bad.files[0].to = "../escape/SKILL.md".into();
    assert!(bad.check().is_err());
    let mut bad = good.clone();
    bad.commit = "main".into();
    assert!(bad.check().is_err());
    let mut bad = good.clone();
    bad.repo = "someone/skills/../other".into();
    assert!(bad.check().is_err());
}
