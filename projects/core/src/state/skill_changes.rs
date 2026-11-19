//! What skills changed during turns, kept with the text before so a change can be undone.
//! Rows are capped per skill and by text size, so the table stays small.

use anyhow::{Context, Result, ensure};
use dusk_protocol::catalog::{SkillChange, SkillSaveAction};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{State, timestamp_ms};

/// Rows kept per skill; older ones are pruned when a new one lands.
pub const MAX_KEPT_PER_SKILL: usize = 20;
/// Largest text stored, the same as the node limit.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

/// A change to record. `before` is `None` when the skill was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSkillChange {
    pub skill: String,
    pub thread_id: String,
    pub call_id: String,
    pub created: bool,
    pub reason: String,
    pub before: Option<String>,
    pub after: String,
}

/// A recorded change. `before` is `None` for a creation or when it was too large to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSkillChange {
    pub id: i64,
    pub skill: String,
    pub thread_id: String,
    pub call_id: String,
    pub created: bool,
    pub reason: String,
    pub before: Option<String>,
    pub after: String,
    pub changed_at_ms: i64,
    pub undone_at_ms: Option<i64>,
}

impl StoredSkillChange {
    /// What a client is told of it; the texts stay in the core.
    pub fn info(&self) -> SkillChange {
        SkillChange {
            id: self.id,
            skill: self.skill.clone(),
            call_id: self.call_id.clone(),
            action: if self.created {
                SkillSaveAction::Created
            } else {
                SkillSaveAction::Updated
            },
            reason: self.reason.clone(),
            changed_at_ms: self.changed_at_ms,
            undone: self.undone_at_ms.is_some(),
            can_undo: self.undone_at_ms.is_none()
                && (self.created || self.before.is_some())
                && crate::catalog::skills::pack_of(&self.skill).as_deref()
                    != Some(crate::catalog::skills::BUILTIN_PACK),
        }
    }
}

fn stored(row: &SqliteRow) -> Result<StoredSkillChange> {
    Ok(StoredSkillChange {
        id: row.try_get("id")?,
        skill: row.try_get("skill")?,
        thread_id: row.try_get("thread_id")?,
        call_id: row.try_get("call_id")?,
        created: row.try_get::<String, _>("action")? == "created",
        reason: row.try_get("reason")?,
        before: row.try_get("before")?,
        after: row.try_get("after")?,
        changed_at_ms: row.try_get("changed_at_ms")?,
        undone_at_ms: row.try_get("undone_at_ms")?,
    })
}

impl State {
    /// Stores one change and returns its id. A `before` over the cap is dropped (the row
    /// then cannot be undone); an `after` over it is refused.
    pub async fn record_skill_change(&self, change: NewSkillChange) -> Result<i64> {
        ensure!(
            change.after.len() <= MAX_TEXT_BYTES,
            "skill text is over {MAX_TEXT_BYTES} bytes"
        );
        let before = change.before.filter(|text| text.len() <= MAX_TEXT_BYTES);
        let action = if change.created { "created" } else { "updated" };
        let changed_at_ms = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin skill change")?;
        let id = sqlx::query(
            "INSERT INTO skill_changes
               (skill, thread_id, call_id, action, reason, before, after, changed_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&change.skill)
        .bind(&change.thread_id)
        .bind(&change.call_id)
        .bind(action)
        .bind(&change.reason)
        .bind(before)
        .bind(&change.after)
        .bind(changed_at_ms)
        .execute(&mut *transaction)
        .await
        .context("insert the skill change")?
        .last_insert_rowid();
        sqlx::query(
            "DELETE FROM skill_changes WHERE skill = ? AND id NOT IN
               (SELECT id FROM skill_changes WHERE skill = ? ORDER BY id DESC LIMIT ?)",
        )
        .bind(&change.skill)
        .bind(&change.skill)
        .bind(MAX_KEPT_PER_SKILL as i64)
        .execute(&mut *transaction)
        .await
        .context("prune old skill changes")?;
        transaction.commit().await.context("commit skill change")?;
        Ok(id)
    }

    /// The changes a thread made, oldest first.
    pub async fn skill_changes(&self, thread_id: &str) -> Result<Vec<StoredSkillChange>> {
        let rows = sqlx::query(
            "SELECT id, skill, thread_id, call_id, action, reason, before, after,
                    changed_at_ms, undone_at_ms
             FROM skill_changes WHERE thread_id = ? ORDER BY id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("read skill changes")?;
        rows.iter().map(stored).collect()
    }

    pub async fn skill_change(&self, id: i64) -> Result<Option<StoredSkillChange>> {
        let row = sqlx::query(
            "SELECT id, skill, thread_id, call_id, action, reason, before, after,
                    changed_at_ms, undone_at_ms
             FROM skill_changes WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("read a skill change")?;
        row.as_ref().map(stored).transpose()
    }

    /// Notes that a change was undone; a second call keeps the first time.
    pub async fn mark_skill_change_undone(&self, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE skill_changes SET undone_at_ms = ? WHERE id = ? AND undone_at_ms IS NULL",
        )
        .bind(timestamp_ms()?)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("mark the skill change undone")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(skill: &str, n: usize) -> NewSkillChange {
        NewSkillChange {
            skill: skill.into(),
            thread_id: "t".into(),
            call_id: format!("c{n}"),
            created: false,
            reason: "r".into(),
            before: Some(format!("before {n}")),
            after: format!("after {n}"),
        }
    }

    #[tokio::test]
    async fn history_keeps_twenty_per_skill() {
        let state = crate::testing::temp_state("skill_changes_cap").await;
        let mut ids = Vec::new();
        for n in 0..25 {
            ids.push(state.record_skill_change(change("a", n)).await.unwrap());
        }
        state.record_skill_change(change("b", 0)).await.unwrap();
        let rows = state.skill_changes("t").await.unwrap();
        let a: Vec<_> = rows.iter().filter(|row| row.skill == "a").collect();
        assert_eq!(a.len(), MAX_KEPT_PER_SKILL);
        for id in &ids[..5] {
            assert!(state.skill_change(*id).await.unwrap().is_none());
        }
        assert_eq!(a[0].id, ids[5]);
        assert_eq!(rows.iter().filter(|row| row.skill == "b").count(), 1);
    }

    #[tokio::test]
    async fn oversized_before_is_not_kept() {
        let state = crate::testing::temp_state("skill_changes_big").await;
        let mut big = change("a", 0);
        big.before = Some("x".repeat(MAX_TEXT_BYTES + 1));
        let id = state.record_skill_change(big).await.unwrap();
        assert_eq!(state.skill_change(id).await.unwrap().unwrap().before, None);
        let mut huge = change("a", 1);
        huge.after = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(state.record_skill_change(huge).await.is_err());
    }

    #[tokio::test]
    async fn undone_is_marked_once() {
        let state = crate::testing::temp_state("skill_changes_undo").await;
        let id = state.record_skill_change(change("a", 0)).await.unwrap();
        state.mark_skill_change_undone(id).await.unwrap();
        let first = state.skill_change(id).await.unwrap().unwrap().undone_at_ms;
        assert!(first.is_some());
        state.mark_skill_change_undone(id).await.unwrap();
        assert_eq!(
            state.skill_change(id).await.unwrap().unwrap().undone_at_ms,
            first
        );
    }
}
