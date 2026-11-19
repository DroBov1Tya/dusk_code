//! What the user edits as files: skills, agents and their problems.

mod agent;
mod node;
mod problem;
mod skill;

pub use agent::{AgentNode, AgentTree};
pub use node::NodeText;
pub use problem::Problem;
pub use skill::{
    DiffKind, DiffLine, PackSkill, SAVE_MARKER, SkillChange, SkillDiff, SkillInfo, SkillKind,
    SkillPack, SkillSave, SkillSaveAction,
};
