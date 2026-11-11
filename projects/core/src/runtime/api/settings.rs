//! The settings a client changes: modes, tools, memory, limits and policies.

use crate::runtime::Runtime;
use crate::{
    catalog::modes::Modes,
    config::ApprovalPolicy,
    release::update::Channel,
    tools::{ToolKind, ToolScope},
};
use anyhow::Result;
use std::sync::Arc;

impl Runtime {
    /// Every mode of the chat, in the order Tab walks them.
    pub fn modes(&self) -> Vec<String> {
        Modes::new(&self.launcher.paths.modes_dir()).list()
    }

    /// Every mode with the first sentence of its prompt, for a client to describe it.
    pub fn mode_infos(&self) -> Vec<dusk_protocol::config::ModeInfo> {
        let dir = self.launcher.paths.modes_dir();
        self.modes()
            .into_iter()
            .map(|name| {
                let prompt =
                    std::fs::read_to_string(dir.join(format!("{name}.md"))).unwrap_or_default();
                let flat = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
                let end = flat.find(". ").map_or(flat.len(), |at| at + 1);
                let summary = flat[..end].chars().take(200).collect();
                dusk_protocol::config::ModeInfo { name, summary }
            })
            .collect()
    }

    /// Moves the chat to the next mode; its prompt and tools apply from the next step.
    pub fn next_mode(&self) -> Result<String> {
        let current = self.config().current().mode().to_owned();
        let next = Modes::new(&self.launcher.paths.modes_dir()).next(&current);
        self.config()
            .update(|config| config.set_mode(next.clone()))?;
        Ok(next)
    }

    /// Moves the chat to the mode named, one of [`Runtime::modes`].
    pub fn set_mode(&self, name: &str) -> Result<()> {
        anyhow::ensure!(
            self.modes().iter().any(|mode| mode == name),
            "no mode is named {name}; /mode lists them"
        );
        self.config()
            .update(|config| config.set_mode(name.to_owned()))?;
        Ok(())
    }

    pub fn set_tool_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher.config.update(|config| {
            let mode = config.mode().to_owned();
            config.set_mode_tool_enabled(&mode, name, enabled);
        })?;
        Ok(())
    }

    pub fn set_memory_enabled(&self, enabled: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_memory_enabled(enabled))?;
        Ok(())
    }

    pub fn set_memory_chars(&self, chars: usize) -> Result<()> {
        self.config()
            .update(|config| config.set_memory_chars(chars))?;
        Ok(())
    }

    pub fn set_work_history_enabled(&self, enabled: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_work_history_enabled(enabled))?;
        Ok(())
    }

    pub fn set_work_history_chars(&self, chars: usize) -> Result<()> {
        self.config()
            .update(|config| config.set_work_history_chars(chars))?;
        Ok(())
    }

    pub fn set_update_channel(&self, channel: Channel) -> Result<()> {
        self.config()
            .update(|config| config.set_update_channel(channel))?;
        Ok(())
    }

    pub fn set_max_agents_at_work(&self, limit: usize) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_max_agents_at_work(limit))?;
        Ok(())
    }

    /// Turns every log and trace of the process off or on again, at once.
    pub fn set_logging_enabled(&self, enabled: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_logging_enabled(enabled))?;
        crate::init::logs::set_enabled(enabled);
        Ok(())
    }

    pub fn set_idle_screen_minutes(&self, minutes: u32) -> Result<()> {
        self.config()
            .update(|config| config.set_idle_screen_minutes(minutes))?;
        Ok(())
    }

    pub fn set_notifications(&self, on: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_notifications(on))?;
        Ok(())
    }

    /// The version control branch prefix; empty adds none.
    pub fn set_branch_prefix(&self, prefix: &str) -> Result<()> {
        self.config()
            .update(|config| config.set_branch_prefix(prefix))?;
        Ok(())
    }

    pub fn set_sound_notifications(&self, on: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_sound_notifications(on))?;
        Ok(())
    }

    /// Full access answers every approval request with yes.
    pub fn set_approvals(&self, policy: ApprovalPolicy) -> Result<()> {
        self.config()
            .update(|config| config.set_approvals(policy))?;
        Ok(())
    }

    pub fn set_max_goal_rounds(&self, rounds: u32) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_max_goal_rounds(rounds))?;
        Ok(())
    }

    /// The tools a chat gets, by name.
    /// Every tool with its kind, by kind and then by name.
    /// Every switchable tool with its kind and what it tells the model.
    pub fn tool_kinds(&self) -> Result<Vec<(String, ToolKind, String)>> {
        // Tools do not depend on the thread.
        let tools = self.launcher.tools("", None, 0);
        let root = self.launcher.paths.workspace_root();
        let scope = Arc::new(ToolScope::new(root, root)?);
        let mut descriptions = tools.descriptions(&scope, &self.launcher.config.current());
        Ok(tools
            .kinds()
            .into_iter()
            .map(|(name, kind)| {
                let description = descriptions.remove(&name).unwrap_or_default();
                (name, kind, description)
            })
            .collect())
    }
}
