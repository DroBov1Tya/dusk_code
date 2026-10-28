//! Client commands that start with `:`. They change settings through the core and never
//! reach the model: `:tools`, `:on <tool>`, `:off <tool>`, `:reload`.

use std::sync::Arc;

use anyhow::{Result, bail};
use dusk_core::live_config::LiveConfig;

pub const PREFIX: char = ':';

pub struct Settings {
    pub config: Arc<LiveConfig>,
    pub tools: Vec<&'static str>,
}

/// Runs a command line without the prefix and returns what to show the user.
pub fn run(line: &str, settings: &Settings) -> Result<String> {
    let mut words = line.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("tools"), None, _) => Ok(tool_list(settings)),
        (Some("on"), Some(tool), None) => switch(settings, tool, true),
        (Some("off"), Some(tool), None) => switch(settings, tool, false),
        (Some("reload"), None, _) => {
            settings.config.reload()?;
            Ok("config reloaded".into())
        }
        _ => bail!("unknown command; use :tools, :on <tool>, :off <tool> or :reload"),
    }
}

fn tool_list(settings: &Settings) -> String {
    let config = settings.config.current();
    settings
        .tools
        .iter()
        .map(|&tool| {
            let state = if config.tool_enabled(tool) {
                "on "
            } else {
                "off"
            };
            format!("{state} {tool}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn switch(settings: &Settings, tool: &str, enabled: bool) -> Result<String> {
    if !settings.tools.contains(&tool) {
        bail!("unknown tool {tool}; :tools lists them");
    }
    settings
        .config
        .update(|config| config.set_tool_enabled(tool, enabled))?;
    let state = if enabled { "on" } else { "off" };
    Ok(format!("{tool} is {state}"))
}
