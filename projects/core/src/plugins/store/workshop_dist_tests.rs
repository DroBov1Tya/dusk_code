//! The Workshop's own packs against this reader. Skipped unless `DUSKCODE_WORKSHOP_DIST`
//! names a folder of built pack archives (`scripts/package-packs.sh`); the optional
//! `DUSKCODE_WORKSHOP_INDEX` is an unsigned format 2 list from `publish.py --dry-run`.
//!
//!   cargo test -p dusk_core workshop_dist -- --ignored

use std::{collections::BTreeSet, fs, path::Path};

use dusk_protocol::plugin::Kind;

use super::{content, index, tests::signed};
use crate::{
    catalog::graph::{AGENT_FILE, Graph, SKILL_FILE, front_skills},
    init::paths::Paths,
    plugins::catalog,
    release::resources,
    testing::temp_paths,
};

/// `name-1.20261117.0.tar.gz` is the pack `name`; a plugin's `-macos-aarch64` is not.
fn pack_name(file: &str) -> Option<&str> {
    let (name, version) = file.strip_suffix(".tar.gz")?.rsplit_once('-')?;
    version
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
        .then_some(name)
}

fn copy_builtin(from: &Path, to: &Path) {
    for relative in resources::files(from).unwrap() {
        fs::create_dir_all(to.join(&relative).parent().unwrap()).unwrap();
        fs::copy(from.join(&relative), to.join(&relative)).unwrap();
    }
}

fn fallback_entry(name: &str) -> index::Entry {
    serde_json::from_value(serde_json::json!({
        "name": name, "version": "1.0.0", "kind": "pack",
        "downloads": [{"os": "any", "arch": "any", "url": "https://example.com/p.tar.gz",
                       "sha256": "0".repeat(64), "size": 1}],
    }))
    .unwrap()
}

#[test]
#[ignore = "needs DUSKCODE_WORKSHOP_DIST"]
fn workshop_dist_installs_and_resolves() {
    let Some(dist) = std::env::var_os("DUSKCODE_WORKSHOP_DIST") else {
        return;
    };
    // The list goes through the real verifier, signed by a throwaway key.
    let listed = std::env::var_os("DUSKCODE_WORKSHOP_INDEX").map(|file| {
        let json = fs::read(file).unwrap();
        let (key, signature) = signed(std::str::from_utf8(&json).unwrap());
        index::verify(&json, &signature, &key, chrono::Utc::now()).unwrap()
    });
    let paths: Paths = temp_paths("workshop-dist");
    fs::create_dir_all(catalog::folder(&paths)).unwrap();

    let mut installed = 0;
    for file in fs::read_dir(dist).unwrap() {
        let file = file.unwrap().path();
        let Some(name) = file
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(pack_name)
        else {
            continue;
        };
        let entry = match &listed {
            Some(list) => match list.plugins.iter().find(|e| e.name == name) {
                Some(entry) => entry.clone(),
                None => continue,
            },
            None => fallback_entry(name),
        };
        assert!(matches!(entry.kind, Kind::Pack | Kind::Agent), "{name}");
        let bytes = fs::read(&file).unwrap();
        content::place(&paths, &entry, &bytes, None)
            .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        installed += 1;
    }
    assert!(installed > 0, "no pack archive found");

    // What a fresh install also has: the built-in skills and agents.
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources");
    copy_builtin(&resources.join("skills"), &paths.skills_dir());
    copy_builtin(&resources.join("agents"), &paths.agents_dir());

    let skills = Graph::load(&paths.skills_dir(), SKILL_FILE);
    let agents = Graph::load(&paths.agents_dir(), AGENT_FILE);
    for graph in [&skills, &agents] {
        assert!(graph.problems().is_empty(), "{:?}", graph.problems());
        assert!(graph.skipped().is_empty(), "{:?}", graph.skipped());
    }
    let have: BTreeSet<&str> = skills.nodes().map(|node| node.name.as_str()).collect();
    let mut missing = Vec::new();
    for agent in agents.nodes() {
        let text = fs::read_to_string(agent.file()).unwrap();
        for skill in front_skills(&text)
            .into_iter()
            .filter(|s| !have.contains(s.as_str()))
        {
            missing.push(format!("{} -> {skill}", agent.name));
        }
    }
    assert!(missing.is_empty(), "skills not found: {missing:#?}");
}
