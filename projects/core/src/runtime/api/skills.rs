//! Skills: switching them and editing their files.

use crate::runtime::Runtime;
use crate::{
    catalog::graph::{NodeText, SKILL_FILE, read_node_text, save_node_text},
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

    pub fn skills(&self) -> Vec<SkillInfo> {
        skills::list(
            &self.launcher.paths.skills_dir(),
            &self.launcher.config.current(),
        )
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
