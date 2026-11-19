//! Skills: switching them and editing their files.

use crate::runtime::Runtime;
use crate::{
    catalog::graph::{NodeText, SKILL_FILE, read_node_text, save_node_text},
    catalog::packs::{self, SkillPack},
    catalog::skills::{self, SkillInfo},
};
use anyhow::Result;

impl Runtime {
    pub fn set_skill_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_skill_enabled(name, enabled))?;
        Ok(())
    }

    /// Switches a skill pack: off hides all its skills, each keeping its own switch.
    pub fn set_pack_enabled(&self, pack: &str, enabled: bool) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_pack_enabled(pack, enabled))?;
        Ok(())
    }

    pub fn skill_packs(&self) -> Vec<SkillPack> {
        packs::list(
            &self.launcher.paths.skills_dir(),
            &self.launcher.config.current(),
        )
    }

    /// Every skill; one the Workshop installed is marked as from there, not the user's, and
    /// one from an Unverified source by that source's name.
    pub fn skills(&self) -> Vec<SkillInfo> {
        let mut skills = skills::list(
            &self.launcher.paths.skills_dir(),
            &self.launcher.config.current(),
        );
        let installed = crate::plugins::store::installed_skills(&self.launcher.paths);
        let unverified = crate::plugins::store::unverified_skills(&self.launcher.paths);
        for skill in &mut skills {
            // Marked whatever its front matter claims: nobody vouches for its source.
            if let Some((_, source)) =
                crate::plugins::store::unverified_source(&unverified, &skill.name)
            {
                skill.kind = skills::SkillKind::Other;
                skill.source = Some(format!("{source}, unverified"));
            } else if skill.kind == skills::SkillKind::Own && installed.contains(&skill.name) {
                skill.kind = skills::SkillKind::Other;
                skill.source = Some("Workshop".to_owned());
            }
        }
        skills
    }

    /// A skill's file as it is on disk; reading it again throws edits away.
    pub fn skill(&self, name: &str) -> Result<NodeText> {
        read_node_text(&self.launcher.paths.skills_dir(), SKILL_FILE, name)
    }

    /// Saves a skill's file once the text reads as a skill.
    pub fn save_skill(&self, name: &str, text: &str) -> Result<NodeText> {
        save_node_text(&self.launcher.paths.skills_dir(), SKILL_FILE, name, text)
    }
}
