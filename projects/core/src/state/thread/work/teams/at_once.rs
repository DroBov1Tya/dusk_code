//! How many members of a running team work at once: the user changes it, within the
//! setting `limits.team_at_once` cut to `max_agents_at_work`.

use dusk_protocol::thread::TeamStatus;

use super::{State, TeamRun, error::TeamError, read_run, root_of};

impl State {
    /// Sets the running team's members at once, 1 to `ceiling`. Members already at work
    /// past a lower number go on; no new one starts until fewer are at work.
    pub async fn set_team_at_once(
        &self,
        thread: &str,
        at_once: u32,
        ceiling: u32,
    ) -> Result<TeamRun, TeamError> {
        if !(1..=ceiling).contains(&at_once) {
            return Err(TeamError::PastCeiling { ceiling });
        }
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if let TeamStatus::Ended(why) = run.status {
            return Err(TeamError::Ended(why));
        }
        let changed =
            sqlx::query("UPDATE teams SET at_once = ? WHERE thread_id = ? AND status = 'running'")
                .bind(i64::from(at_once))
                .bind(&root)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if changed != 1 {
            return Err(TeamError::Store(anyhow::anyhow!(
                "the team changed while its members at once were set"
            )));
        }
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        transaction.commit().await?;
        Ok(run)
    }
}
