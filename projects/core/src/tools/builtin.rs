//! The built-in tools.

use std::path::Path;

use super::{
    console::RunCommand,
    files::{DeleteFile, EditFile, ReadFile, SearchFiles, WriteFile},
    harness::LoadSkill,
    tool::Tool,
};

/// `skills` is the directory the skill graph is read from.
pub fn all(skills: &Path) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(SearchFiles),
        Box::new(WriteFile),
        Box::new(EditFile),
        Box::new(DeleteFile),
        Box::new(RunCommand::default()),
        Box::new(LoadSkill::new(skills.to_owned())),
    ]
}
