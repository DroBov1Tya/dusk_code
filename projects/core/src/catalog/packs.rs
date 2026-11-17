//! Skill packs as a client switches them: the skills of `skills::list` grouped by
//! `skills::pack_of`, with each pack's title from its `pack.yml` when it has one.

use std::{collections::BTreeMap, path::Path};

use crate::{
    catalog::skills::{self, BUILTIN_PACK},
    config::Config,
    plugins::store::Pack,
};

pub use dusk_protocol::catalog::{PackSkill, SkillPack};

const MAX_PACK_FILE: u64 = 64 * 1024;

/// Every pack that holds a skill, the built-in one first, then by name.
pub fn list(directory: &Path, config: &Config) -> Vec<SkillPack> {
    let mut packs: BTreeMap<(bool, String), SkillPack> = BTreeMap::new();
    for skill in skills::list(directory, config) {
        let Some(name) = skills::pack_of(&skill.name) else {
            continue;
        };
        let builtin = name == BUILTIN_PACK;
        let pack = packs
            .entry((!builtin, name.clone()))
            .or_insert_with(|| about(directory, &name, builtin, config));
        pack.skills.push(PackSkill {
            enabled: config.skill_switch(&skill.name),
            name: skill.name,
        });
    }
    packs.into_values().collect()
}

fn about(directory: &Path, name: &str, builtin: bool, config: &Config) -> SkillPack {
    let file = directory.join(name).join(crate::plugins::store::PACK_FILE);
    // A pack the store did not install, or a damaged pack.yml, still lists by its name.
    let pack = (!builtin)
        .then(|| crate::init::files::read_regular(&file, MAX_PACK_FILE).ok())
        .flatten()
        .and_then(|bytes| Pack::parse(&bytes).ok());
    SkillPack {
        name: name.to_owned(),
        title: pack
            .as_ref()
            .map_or_else(String::new, |pack| pack.title.clone()),
        description: pack.map_or_else(String::new, |pack| pack.description),
        builtin,
        enabled: config.pack_enabled(name),
        skills: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn write(root: &Path, name: &str) {
        fs::create_dir_all(root.join(name)).unwrap();
        let text = format!("---\ndescription: {name}\n---\nDo it.");
        fs::write(root.join(name).join("SKILL.md"), text).unwrap();
    }

    #[test]
    fn a_switched_off_pack_or_skill_is_hidden() {
        let root = crate::testing::temp_dir("skill-packs");
        for name in ["eng/spike", "eng/tdd", "mine"] {
            write(&root, name);
        }
        let mut config = Config::parse(b"{}").unwrap();
        let shown = |config: &Config| -> Vec<String> {
            skills::list(&root, config)
                .into_iter()
                .filter(|skill| config.skill_enabled(&skill.name))
                .map(|skill| skill.name)
                .collect()
        };
        assert_eq!(shown(&config), ["eng/spike", "eng/tdd", "mine"]);

        config.set_pack_enabled("eng", false);
        assert_eq!(shown(&config), ["mine"]);
        let eng = list(&root, &config);
        assert_eq!(eng.len(), 1, "a skill of its own is in no pack");
        assert!(!eng[0].enabled && eng[0].skills.iter().all(|skill| skill.enabled));

        config.set_pack_enabled("eng", true);
        config.set_skill_enabled("eng/tdd", false);
        assert_eq!(shown(&config), ["eng/spike", "mine"]);

        // The built-in skills are the pack `duskcode`.
        assert!(config.skill_enabled("skill-creator"));
        config.set_pack_enabled(BUILTIN_PACK, false);
        assert!(!config.skill_enabled("skill-creator"));
        assert!(config.skill_enabled("mine"));
    }
}
