//! Skills as the user edits them.

use serde::{Deserialize, Serialize};

pub const SAVE_MARKER: &str = "[dusk_skill_saved] ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SkillKind {
    Builtin,
    Own,
    Other,
}

impl SkillKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Builtin => "built-in",
            Self::Own => "own",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: SkillKind,
    /// The company or project it comes from, as its front matter says.
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    /// Why the skill cannot be used as it is; it can still be opened and fixed.
    pub problem: Option<String>,
}

/// A successful file-tool save. Kept in its stored result so replay needs no filesystem
/// inspection and cannot mistake a loaded skill for an updated one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSave {
    pub name: String,
    pub action: SkillSaveAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSaveAction {
    Created,
    Updated,
    Unchanged,
}

impl SkillSave {
    pub fn stored(&self) -> String {
        format!("{SAVE_MARKER}{}", serde_json::to_string(self).unwrap())
    }

    /// Only file-tool results are eligible; callers also check the tool's success.
    pub fn from_result(content: &str) -> Option<Self> {
        serde_json::from_str(content.lines().last()?.strip_prefix(SAVE_MARKER)?).ok()
    }

    pub fn notice(&self) -> String {
        let action = match self.action {
            SkillSaveAction::Created => "created",
            SkillSaveAction::Updated => "updated",
            SkillSaveAction::Unchanged => "saved without changes",
        };
        let name: String = self
            .name
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        format!("Skill {action}: {name}")
    }
}
