//! What is switched on or off: tools, modes, agents and skills.

use super::Config;
use crate::{
    catalog::{
        modes::{self, Modes},
        skills,
    },
    tools::ToolKind,
};

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
        if mode == modes::MAIN {
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

    /// Whether a skill may be loaded: its own switch and its pack's are on.
    pub fn skill_enabled(&self, name: &str) -> bool {
        self.skill_switch(name) && skills::pack_of(name).is_none_or(|pack| self.pack_enabled(&pack))
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
