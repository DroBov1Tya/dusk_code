//! Skills, packs and agents from the store: what their packages may hold and where
//! they land.

use std::fs;

use tar::EntryType::{Directory, Regular};

use super::{content, index::Entry, pack::Pack, tests::package};
use crate::{init::paths::Paths, plugins::catalog, testing::temp_paths};

fn entry(name: &str, kind: &str, extra: &str) -> Entry {
    let json = format!(
        r#"{{"name": "{name}", "version": "1.0.0", "kind": "{kind}", "downloads": [{{"os": "any", "arch": "any", "url": "https://example.com/p.tar.gz", "sha256": "{}", "size": 10}}]{extra}}}"#,
        super::tests::SHA
    );
    serde_json::from_str(&json).unwrap()
}

fn paths(name: &str) -> Paths {
    let paths = temp_paths(name);
    crate::init::create_private_dir(&catalog::folder(&paths)).unwrap();
    paths
}

const PACK: &str = "name: eng\ntitle: Engineering\ndescription: Ways of work\nvendor: duskcode\n\
kind: skills\nitems: [spike, tdd]\nlicense: MIT\n";

fn pack_package(pack_yml: &str) -> Vec<u8> {
    package(&[
        ("pack.yml", Regular, pack_yml.as_bytes()),
        ("spike/", Directory, b""),
        (
            "spike/SKILL.md",
            Regular,
            b"---\ndescription: spike\n---\nTry it.",
        ),
        (
            "tdd/SKILL.md",
            Regular,
            b"---\ndescription: tdd\n---\nTest first.",
        ),
        ("tdd/references/cycle.md", Regular, b"red, green"),
    ])
}

#[test]
fn pack_yml_is_strict() {
    let pack = Pack::parse(PACK.as_bytes()).unwrap();
    assert_eq!((pack.name.as_str(), pack.items.len()), ("eng", 2));
    for bad in [
        format!("{PACK}extra: 1\n"),
        PACK.replace("name: eng", "name: Eng"),
        PACK.replace("[spike, tdd]", "[spike, ../x]"),
        PACK.replace("[spike, tdd]", "[spike, spike]"),
        PACK.replace("[spike, tdd]", "[]"),
        PACK.replace("kind: skills", "kind: plugins"),
        PACK.replace("license: MIT\n", ""),
        format!("{PACK}upstream: http://example.com\n"),
    ] {
        assert!(Pack::parse(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn only_plain_files_are_for_any_system() {
    assert!(entry("eng", "pack", "").download_here().is_ok());
    let plugin = entry("bot", "plugin", "");
    let error = plugin.download_here().unwrap_err();
    assert!(error.contains("any system"), "{error}");
    // A skill without a package for any system has none here.
    let json = r#"{"name": "x", "version": "1", "kind": "skill", "downloads": [{"os": "macos", "url": "https://e.com/p", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 1}]}"#;
    let skill: Entry = serde_json::from_str(json).unwrap();
    assert!(skill.download_here().is_err());
}

#[test]
fn packages_land_in_their_folders() {
    let paths = paths("content-place");
    let rules = paths.rules_root();
    let skill = package(&[(
        "review/SKILL.md",
        Regular,
        b"---\ndescription: r\n---\nRead.",
    )]);
    let review = entry("review", "skill", "");
    let folder = content::place(&paths, &review, &skill, None).unwrap();
    assert_eq!(folder, "skills/review");
    assert!(rules.join("skills/review/SKILL.md").is_file());

    let eng = entry("eng", "pack", r#", "skills": ["spike", "tdd"]"#);
    let folder = content::place(&paths, &eng, &pack_package(PACK), None).unwrap();
    assert_eq!(folder, "skills/eng");
    assert!(rules.join("skills/eng/tdd/references/cycle.md").is_file());
    assert!(rules.join("skills/eng/pack.yml").is_file());

    let agents = PACK
        .replace("kind: skills", "kind: agents")
        .replace("[spike, tdd]", "[lead]")
        .replace("eng", "crew");
    let crew = package(&[
        ("pack.yml", Regular, agents.as_bytes()),
        (
            "lead/AGENT.md",
            Regular,
            b"---\ndescription: lead\n---\nLead.",
        ),
    ]);
    let folder = content::place(&paths, &entry("crew", "agent", ""), &crew, None).unwrap();
    assert_eq!(folder, "agents/crew");
    assert!(rules.join("agents/crew/lead/AGENT.md").is_file());

    // A folder already there is replaced only by the store that put it there.
    assert!(content::place(&paths, &review, &skill, None).is_err());
    content::place(&paths, &review, &skill, Some("skills/review")).unwrap();
}

#[test]
fn a_package_out_of_its_shape_is_refused() {
    let paths = paths("content-refused");
    let rules = paths.rules_root();
    let eng = entry("eng", "pack", r#", "skills": ["spike", "tdd"]"#);
    let review = entry("review", "skill", "");
    for (why, listed, bytes) in [
        (
            "traversal",
            &review,
            package(&[("../x/SKILL.md", Regular, b"x")]),
        ),
        (
            "other folder",
            &review,
            package(&[("other/SKILL.md", Regular, b"x")]),
        ),
        (
            "no SKILL.md",
            &review,
            package(&[("review/README.md", Regular, b"x")]),
        ),
        (
            "no pack.yml",
            &eng,
            package(&[("spike/SKILL.md", Regular, b"x")]),
        ),
        (
            "other name",
            &eng,
            pack_package(&PACK.replace("name: eng", "name: other")),
        ),
        (
            "other items",
            &eng,
            pack_package(&PACK.replace("tdd]", "tdd, more]")),
        ),
        (
            "a stray file",
            &eng,
            package(&[
                ("pack.yml", Regular, PACK.as_bytes()),
                ("spike/SKILL.md", Regular, b"x"),
                ("tdd/SKILL.md", Regular, b"x"),
                ("run.sh", Regular, b"#!/bin/sh"),
            ]),
        ),
    ] {
        assert!(
            content::place(&paths, listed, &bytes, None).is_err(),
            "{why}"
        );
    }
    assert!(!rules.join("x").exists() && !rules.join("skills/x").exists());
    assert!(!rules.join("skills/eng").exists() && !rules.join("skills/review").exists());
}

#[test]
fn removing_a_pack_keeps_what_the_user_edited() {
    let paths = paths("content-remove");
    let skills = paths.rules_root().join("skills/eng");
    content::place(&paths, &entry("eng", "pack", ""), &pack_package(PACK), None).unwrap();
    fs::write(skills.join("spike/SKILL.md"), "my own spike").unwrap();
    content::remove(&paths, "eng").unwrap();
    assert_eq!(
        fs::read_to_string(skills.join("spike/SKILL.md")).unwrap(),
        "my own spike"
    );
    assert!(
        !skills.join("tdd").exists(),
        "untouched files and their folders go"
    );
    assert!(!skills.join("pack.yml").exists());
}

#[test]
fn a_single_skill_installs_beside_the_others_and_never_onto_a_pack() {
    use super::record::{Installed, Record};
    use crate::{catalog::skills, config::Config};

    let paths = paths("content-single");
    let skill = package(&[(
        "triage/SKILL.md",
        Regular,
        b"---\ndescription: Sort the reports first.\n---\nSort.",
    )]);
    let triage = entry("triage", "skill", "");
    let folder = content::place(&paths, &triage, &skill, None).unwrap();
    Record::update(&paths, |record| {
        record.installed.insert(
            "triage".into(),
            Installed {
                version: "1.0.0".into(),
                sha256: super::tests::SHA.into(),
                source: "official".into(),
                url: String::new(),
                key: String::new(),
                kind: dusk_protocol::plugin::Kind::Skill,
                folder,
                author: String::new(),
                license: String::new(),
                origin: String::new(),
                target: String::new(),
            },
        );
        Ok(())
    })
    .unwrap();
    let config = Config::parse(b"{}").unwrap();
    let listed = skills::list(&paths.skills_dir(), &config);
    let shown = listed.iter().find(|item| item.name == "triage").unwrap();
    assert!(shown.enabled && shown.description == "Sort the reports first.");
    assert!(super::installed_skills(&paths).contains("triage"));

    // A skill named as a pack here lands in the pack's folder, so it is refused; the
    // reverse too.
    content::place(&paths, &entry("eng", "pack", ""), &pack_package(PACK), None).unwrap();
    let eng = package(&[("eng/SKILL.md", Regular, b"---\ndescription: e\n---\nx")]);
    assert!(content::place(&paths, &entry("eng", "skill", ""), &eng, None).is_err());
    let pack = pack_package(&PACK.replace("name: eng", "name: triage"));
    assert!(content::place(&paths, &entry("triage", "pack", ""), &pack, None).is_err());
    assert!(paths.rules_root().join("skills/eng/pack.yml").is_file());
}
