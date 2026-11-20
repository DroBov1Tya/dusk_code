//! Skills as the user edits them.

use serde::{Deserialize, Serialize};

pub const SAVE_MARKER: &str = "[dusk_skill_saved] ";

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// Skills as they came, switched on or off together: the built-in skills as the pack
/// `duskcode`, and every folder under `skills/` that holds skills, such as a pack from
/// the store or a plugin's skills. A skill of the user's own at the top is in none.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPack {
    pub name: String,
    /// From its `pack.yml`; empty when it has none.
    pub title: String,
    pub description: String,
    pub builtin: bool,
    /// The pack's own switch; off hides every skill in it.
    pub enabled: bool,
    pub skills: Vec<PackSkill>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSkill {
    /// The skill's full name, such as `engineering/spike`.
    pub name: String,
    /// The skill's own switch, whatever the pack's.
    pub enabled: bool,
}

/// A successful file-tool save. Kept in its stored result so replay needs no filesystem
/// inspection and cannot mistake a loaded skill for an updated one.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSave {
    pub name: String,
    pub action: SkillSaveAction,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSaveAction {
    Created,
    Updated,
    /// The file is gone; its text is the change's `before`.
    Deleted,
}

impl SkillSave {
    pub fn stored(&self) -> String {
        format!("{SAVE_MARKER}{}", serde_json::to_string(self).unwrap())
    }

    /// Only file-tool results are eligible; callers also check the tool's success.
    pub fn from_result(content: &str) -> Option<Self> {
        serde_json::from_str(content.lines().last()?.strip_prefix(SAVE_MARKER)?).ok()
    }
}

/// One change an agent made to a skill file, kept so it can be seen and undone.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillChange {
    pub id: i64,
    pub skill: String,
    /// The tool call that made it, in its thread.
    pub call_id: String,
    pub action: SkillSaveAction,
    /// One plain line, as the agent gave it.
    pub reason: String,
    pub changed_at_ms: i64,
    pub undone: bool,
    /// Not undone, and the text before it is known (or it created the skill).
    pub can_undo: bool,
}

impl SkillSaveAction {
    /// How a change's row says it: "created", "improved", "deleted".
    pub fn verb(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "improved",
            Self::Deleted => "deleted",
        }
    }
}

impl SkillChange {
    /// The words of its row: "Skill deploy improved: faster", without the colon when no
    /// reason was given. The name and reason are the agent's text: `clean` makes each
    /// plain for the client, and only the reason's first line is shown.
    pub fn label(&self, clean: impl Fn(&str) -> String) -> String {
        let skill = clean(&self.skill);
        let reason = clean(self.reason.lines().next().unwrap_or_default());
        let verb = self.action.verb();
        match reason.trim() {
            "" => format!("Skill {skill} {verb}"),
            reason => format!("Skill {skill} {verb}: {reason}"),
        }
    }
}

/// A change as lines. The text is skill data: shown as plain text, never as markup.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDiff {
    pub change: SkillChange,
    pub lines: Vec<DiffLine>,
    /// More lines followed that are not here.
    pub cut: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", schemars(rename = "SkillDiffLine"))]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", schemars(rename = "SkillDiffKind"))]
pub enum DiffKind {
    Same,
    Added,
    Removed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(action: SkillSaveAction, reason: &str) -> SkillChange {
        SkillChange {
            id: 1,
            skill: "deploy".into(),
            call_id: "c".into(),
            action,
            reason: reason.into(),
            changed_at_ms: 0,
            undone: false,
            can_undo: true,
        }
    }

    #[test]
    fn a_label_shows_the_first_reason_line_and_no_colon_without_one() {
        let same = |text: &str| text.to_owned();
        let label = |action, reason| change(action, reason).label(same);
        assert_eq!(
            label(SkillSaveAction::Created, "adds a step"),
            "Skill deploy created: adds a step"
        );
        assert_eq!(
            label(SkillSaveAction::Updated, "  faster \nsecond"),
            "Skill deploy improved: faster"
        );
        assert_eq!(label(SkillSaveAction::Deleted, ""), "Skill deploy deleted");
        assert_eq!(
            label(SkillSaveAction::Updated, "   "),
            "Skill deploy improved"
        );
        assert_eq!(
            label(SkillSaveAction::Updated, "\nsecond line"),
            "Skill deploy improved",
            "an empty first line is no reason"
        );
        // The client's filter runs before the blank check, on the name too.
        let drop_bidi = |text: &str| text.replace('\u{202E}', "");
        let mut odd = change(SkillSaveAction::Updated, "\u{202E}");
        odd.skill = "de\u{202E}ploy".into();
        assert_eq!(odd.label(drop_bidi), "Skill deploy improved");
    }
}
