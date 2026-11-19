//! What is switched on or off: tools, modes, agents and skills.

use std::path::Path;

use super::Config;
use crate::{
    catalog::{
        modes::{self, Modes},
        skills,
    },
    tools::ToolKind,
};

/// Tools off in every mode until `tools:` in the config switches them on by name:
/// `physical_input` moves the user's real mouse, which only they may allow.
const OFF_UNTIL_SWITCHED_ON: [&str; 1] = ["physical_input"];

impl Config {
    /// Whether the file switches the tool on, whatever the mode; the tests read it back.
    #[cfg(test)]
    pub fn tool_enabled(&self, name: &str) -> bool {
        self.tools.get(name).copied().unwrap_or(true)
    }

    pub fn set_tool_enabled(&mut self, name: &str, enabled: bool) {
        self.tools.insert(name.to_owned(), enabled);
    }

    pub fn mode(&self) -> &str {
        self.mode.as_deref().unwrap_or(modes::MAIN)
    }

    pub fn set_mode(&mut self, mode: String) {
        self.mode = (mode != modes::MAIN).then_some(mode);
    }

    /// A tool's switch in a mode: the user's, else the mode's default, else MAIN's.
    pub fn mode_tool_enabled(&self, mode: &str, name: &str, kind: ToolKind) -> bool {
        // A mode's own default or switch cannot turn on what the user has not asked for.
        if OFF_UNTIL_SWITCHED_ON.contains(&name) && self.tools.get(name) != Some(&true) {
            return false;
        }
        let main = || {
            self.tools
                .get(name)
                .copied()
                .unwrap_or(kind.on_by_default())
        };
        if mode == modes::MAIN {
            return main();
        }
        let switched = self.modes.get(mode).and_then(|tools| tools.get(name));
        switched
            .copied()
            .or_else(|| Modes::default_tool(mode, name, kind))
            .unwrap_or_else(main)
    }

    pub fn set_mode_tool_enabled(&mut self, mode: &str, name: &str, enabled: bool) {
        // One switch for every mode: the user allows it, not a mode.
        if mode == modes::MAIN || OFF_UNTIL_SWITCHED_ON.contains(&name) {
            return self.set_tool_enabled(name, enabled);
        }
        let tools = self.modes.entry(mode.to_owned()).or_default();
        tools.insert(name.to_owned(), enabled);
    }

    pub fn agent_enabled(&self, name: &str) -> bool {
        self.agents.get(name).copied().unwrap_or(true)
    }

    pub fn set_agent_enabled(&mut self, name: &str, enabled: bool) {
        self.agents.insert(name.to_owned(), enabled);
    }

    /// Whether a skill may be loaded: its own switch and its pack's are on. For one skill;
    /// a caller asking about many resolves the built-in folder once for `skill_enabled_in`.
    pub fn skill_enabled(&self, name: &str) -> bool {
        self.skill_enabled_in(name, skills::builtin_dir().as_deref())
    }

    /// `skill_enabled` with the built-in skills' folder already resolved: resolving it
    /// validates the release resources, so a caller asking about many skills does it once.
    pub(crate) fn skill_enabled_in(&self, name: &str, builtin: Option<&Path>) -> bool {
        self.skill_switch(name)
            && skills::pack_in(name, builtin).is_none_or(|pack| self.pack_enabled(&pack))
    }

    /// The skill's own switch, whatever its pack's.
    pub fn skill_switch(&self, name: &str) -> bool {
        self.skills.get(name).copied().unwrap_or(true)
    }

    pub fn pack_enabled(&self, pack: &str) -> bool {
        self.skill_packs.get(pack).copied().unwrap_or(true)
    }

    pub fn set_pack_enabled(&mut self, pack: &str, enabled: bool) {
        self.skill_packs.insert(pack.to_owned(), enabled);
    }

    pub fn set_skill_enabled(&mut self, name: &str, enabled: bool) {
        self.skills.insert(name.to_owned(), enabled);
    }
}
