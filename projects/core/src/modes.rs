//! Modes of the chat: each is a prompt in `modes/<NAME>.md` and its own set of tools.
//! MAIN does the work itself, PLAN only reads and plans, ORCHESTRATOR gives the work
//! to agents. Any other file in the folder is a mode of the user's, with every tool of
//! MAIN. A mode's tools start from the defaults here and can be switched in /settings.
//! Modes apply to the chat only; agents keep their own prompts and tools.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{paths::is_plain_folder_name, thread_start::read_prompt, tools::ToolKind};

pub const MAIN: &str = "MAIN";
pub const PLAN: &str = "PLAN";
pub const ORCHESTRATOR: &str = "ORCHESTRATOR";

/// The built-in modes, in the order Tab walks them.
const BUILTIN: [&str; 3] = [MAIN, PLAN, ORCHESTRATOR];

/// What PLAN can do: read, look things up, and write the plan down.
const PLAN_TOOLS: [&str; 5] = [
    "read_file",
    "read_history",
    "load_skill",
    "tasks",
    "ask_user",
];

/// What ORCHESTRATOR can do: hand work out and keep track of it.
const ORCHESTRATOR_TOOLS: [&str; 5] = [
    "ask_agent",
    "read_history",
    "load_skill",
    "tasks",
    "ask_user",
];

pub const MAIN_TEMPLATE: &str = "\
You are the main agent of dusk_code, working with the user in a chat.
Answer directly and concisely, in the language the user writes in.
";

pub const PLAN_TEMPLATE: &str = "\
You are in plan mode. Do not change anything: no edits, no commands, no agents. Read the
files and the history you need, ask the user when a decision is theirs, and write the
plan as tasks with the tasks tool: small, verifiable steps in order, each with how it
is checked. End with the plan and the open questions; the user switches the mode to
carry it out. Answer in the language the user writes in.
";

pub const ORCHESTRATOR_TEMPLATE: &str = "\
You are the orchestrator. You do no work yourself and assume nothing about the code:
every task goes to an agent. Split the user's request into tasks, give each to the
agent whose description fits it best, and to the general agent when none does. Brief
each agent fully, since it knows only what you write. Check what comes back, keep the
tasks board current, and report to the user in the language they write in.
";

/// The file of a mode's prompt.
pub fn file(directory: &Path, mode: &str) -> PathBuf {
    directory.join(format!("{mode}.md"))
}

/// Every mode: the built-in ones first, then the user's own by name.
pub fn list(directory: &Path) -> Vec<String> {
    let mut own: Vec<String> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let mode = name.strip_suffix(".md")?;
            let own = is_plain_folder_name(mode) && !BUILTIN.contains(&mode);
            (own && entry.path().is_file()).then(|| mode.to_owned())
        })
        .collect();
    own.sort();
    BUILTIN
        .iter()
        .map(|mode| (*mode).to_owned())
        .chain(own)
        .collect()
}

/// The mode after `mode`, round to the first.
pub fn next(directory: &Path, mode: &str) -> String {
    let modes = list(directory);
    let at = modes.iter().position(|known| known == mode);
    let next = at.map_or(0, |at| (at + 1) % modes.len());
    modes[next].clone()
}

/// The prompt of a mode; a missing file of a built-in mode is its template.
pub fn prompt(directory: &Path, mode: &str) -> Result<Option<String>> {
    if let Some(prompt) = read_prompt(&file(directory, mode))? {
        return Ok(Some(prompt));
    }
    Ok(template(mode).map(str::to_owned))
}

pub fn template(mode: &str) -> Option<&'static str> {
    match mode {
        MAIN => Some(MAIN_TEMPLATE),
        PLAN => Some(PLAN_TEMPLATE),
        ORCHESTRATOR => Some(ORCHESTRATOR_TEMPLATE),
        _ => None,
    }
}

/// Whether a tool is on in a mode before the user switches it. `None` follows MAIN's
/// switches, as the user's own modes do.
pub fn default_tool(mode: &str, name: &str, kind: ToolKind) -> Option<bool> {
    let allowed: &[&str] = match mode {
        PLAN => &PLAN_TOOLS,
        ORCHESTRATOR => &ORCHESTRATOR_TOOLS,
        _ => return None,
    };
    Some(kind != ToolKind::Mcp && allowed.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn built_in_modes_come_first_and_tab_goes_round() {
        let directory = temp_dir("modes");
        fs::create_dir_all(&directory).unwrap();
        fs::write(file(&directory, "REVIEW"), "Review only.").unwrap();
        fs::write(directory.join("notes.txt"), "not a mode").unwrap();
        assert_eq!(list(&directory), [MAIN, PLAN, ORCHESTRATOR, "REVIEW"]);
        assert_eq!(next(&directory, "REVIEW"), MAIN);
        assert_eq!(next(&directory, "gone"), MAIN);
        assert_eq!(
            prompt(&directory, PLAN).unwrap().as_deref(),
            Some(PLAN_TEMPLATE)
        );
        assert_eq!(
            default_tool(PLAN, "run_command", ToolKind::Console),
            Some(false)
        );
        assert_eq!(default_tool(PLAN, "read_file", ToolKind::Files), Some(true));
        assert_eq!(
            default_tool("REVIEW", "run_command", ToolKind::Console),
            None
        );
    }
}
