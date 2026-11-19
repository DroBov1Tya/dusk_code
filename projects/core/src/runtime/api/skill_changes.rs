//! What agents changed in skills: listing it, reading a change as lines, undoing it.

use anyhow::{Result, anyhow, bail, ensure};
use dusk_protocol::catalog::{SkillChange, SkillDiff};

use crate::catalog::graph::{SKILL_FILE, node_file, read_node_text, save_node_text};
use crate::catalog::history;
use crate::catalog::skills::{BUILTIN_PACK, pack_of};
use crate::runtime::Runtime;

/// Said when the file is no longer as the change left it.
const CHANGED_SINCE: &str = "the skill changed since; open it in the settings to edit";

impl Runtime {
    /// The changes a thread's agents made, oldest first.
    pub async fn skill_changes(&self, thread_id: &str) -> Result<Vec<SkillChange>> {
        // Always the main state: every change is kept there, also a temporary chat's.
        let changes = self.launcher.state.skill_changes(thread_id).await?;
        Ok(changes.iter().map(|change| change.info()).collect())
    }

    /// A change as lines; a created skill is a diff from nothing.
    pub async fn skill_diff(&self, id: i64) -> Result<SkillDiff> {
        let change = self.stored_skill_change(id).await?;
        let before = change.before.as_deref().unwrap_or("");
        let (lines, cut) = history::diff(before, &change.after);
        Ok(SkillDiff {
            change: change.info(),
            lines,
            cut,
        })
    }

    /// Puts the skill back as it was, only while the file is still as the change left it.
    pub async fn undo_skill_change(&self, id: i64) -> Result<SkillChange> {
        let change = self.stored_skill_change(id).await?;
        ensure!(
            change.undone_at_ms.is_none(),
            "the change was undone already"
        );
        // Agents are not meant to edit built-in skills; a change recorded anyway is shown
        // but cannot be undone.
        ensure!(
            pack_of(&change.skill).as_deref() != Some(BUILTIN_PACK),
            "built-in skills are not changed by agents"
        );
        let directory = self.launcher.paths.skills_dir();
        let now = read_node_text(&directory, SKILL_FILE, &change.skill).ok();
        ensure!(
            now.is_some_and(|node| node.text == change.after),
            "{CHANGED_SINCE}"
        );
        if change.created {
            // Other files of the folder are the user's; the folder goes only when empty.
            let file = node_file(&directory, SKILL_FILE, &change.skill)?;
            std::fs::remove_file(&file)?;
            if let Some(folder) = file.parent() {
                let _ = std::fs::remove_dir(folder);
            }
        } else {
            let Some(before) = change.before.as_deref() else {
                bail!("the text before was too large to keep");
            };
            save_node_text(&directory, SKILL_FILE, &change.skill, before)?;
        }
        self.launcher.state.mark_skill_change_undone(id).await?;
        Ok(self.stored_skill_change(id).await?.info())
    }

    /// The thread a change belongs to, to tell its clients.
    pub async fn skill_change_thread(&self, id: i64) -> Result<String> {
        Ok(self.stored_skill_change(id).await?.thread_id)
    }

    async fn stored_skill_change(&self, id: i64) -> Result<crate::state::StoredSkillChange> {
        self.launcher
            .state
            .skill_change(id)
            .await?
            .ok_or_else(|| anyhow!("no such skill change"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{init::paths::Paths, state::NewSkillChange, testing::temp_dir};

    fn skill(text: &str) -> String {
        format!("---\nname: demo\ndescription: Demo\n---\n{text}\n")
    }

    async fn rig(name: &str) -> (Runtime, std::path::PathBuf) {
        let root = temp_dir(name);
        let paths = Paths::new(root.join("config"), root.join("work"));
        let skills = paths.skills_dir();
        let runtime = Runtime::start(paths, Some(root.join("state.db")))
            .await
            .unwrap();
        runtime.state().create_thread("t", "test").await.unwrap();
        (runtime, skills)
    }

    async fn record(runtime: &Runtime, created: bool, before: Option<&str>, after: &str) -> i64 {
        runtime
            .state()
            .record_skill_change(NewSkillChange {
                skill: "demo".into(),
                thread_id: "t".into(),
                call_id: "c".into(),
                created,
                reason: "why".into(),
                before: before.map(str::to_owned),
                after: after.to_owned(),
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn undo_restores_the_previous_text() {
        let (runtime, skills) = rig("undo_restore").await;
        std::fs::create_dir_all(skills.join("demo")).unwrap();
        let (a, b) = (skill("one"), skill("two"));
        std::fs::write(skills.join("demo").join(SKILL_FILE), &b).unwrap();
        let id = record(&runtime, false, Some(&a), &b).await;
        assert!(runtime.skill_changes("t").await.unwrap()[0].can_undo);
        let info = runtime.undo_skill_change(id).await.unwrap();
        assert!(info.undone && !info.can_undo);
        let text = std::fs::read_to_string(skills.join("demo").join(SKILL_FILE)).unwrap();
        assert_eq!(text, a);
    }

    #[tokio::test]
    async fn undo_refuses_a_change_made_over() {
        let (runtime, skills) = rig("undo_over").await;
        std::fs::create_dir_all(skills.join("demo")).unwrap();
        let (v1, v2, v3) = (skill("one"), skill("two"), skill("three"));
        std::fs::write(skills.join("demo").join(SKILL_FILE), &v3).unwrap();
        let first = record(&runtime, false, Some(&v1), &v2).await;
        let second = record(&runtime, false, Some(&v2), &v3).await;
        let error = runtime.undo_skill_change(first).await.unwrap_err();
        assert!(error.to_string().contains(CHANGED_SINCE));
        runtime.undo_skill_change(second).await.unwrap();
        let text = std::fs::read_to_string(skills.join("demo").join(SKILL_FILE)).unwrap();
        assert_eq!(text, v2);
    }

    #[tokio::test]
    async fn undo_of_a_creation_keeps_other_files() {
        let (runtime, skills) = rig("undo_created").await;
        let folder = skills.join("demo");
        std::fs::create_dir_all(folder.join("references")).unwrap();
        let text = skill("new");
        std::fs::write(folder.join(SKILL_FILE), &text).unwrap();
        std::fs::write(folder.join("references/x.md"), "x").unwrap();
        let id = record(&runtime, true, None, &text).await;
        runtime.undo_skill_change(id).await.unwrap();
        assert!(!folder.join(SKILL_FILE).exists());
        assert!(folder.join("references/x.md").is_file());
    }

    #[test]
    fn diff_lines_are_plain_and_capped() {
        let after = "a\u{1b}b\u{202e}c\n".repeat(1000);
        let (lines, cut) = history::diff("", &after);
        assert!(lines.len() <= 400 && cut);
        assert!(
            lines
                .iter()
                .all(|line| { !line.text.chars().any(|c| c.is_control() || c == '\u{202e}') })
        );
    }
}
