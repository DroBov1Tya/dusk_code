//! The built-in tools.

use std::path::Path;

use super::{
    console::RunCommand,
    files::{Changes, ProjectGraph, ReadFile, SearchFiles},
    harness::{LoadSkill, RecallTool},
    tool::Tool,
};

/// `skills` is the directory the skill graph is read from.
pub fn all(skills: &Path) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(SearchFiles),
        Box::new(Changes),
        Box::new(ProjectGraph),
        Box::new(RecallTool),
        Box::new(RunCommand::default()),
        Box::new(LoadSkill::new(skills.to_owned())),
    ]
}
