//! The pull requests of a chat: those a message of the user or the model links, found by
//! looking through each chat's new messages once, and kept with the last state the forge
//! told after they are merged or closed. The forge reads live in `client::serve`.

use anyhow::{Context, Result};
use dusk_protocol::project::{PullState, PullStatus, ThreadPull, pull_numbers};
use sqlx::Row;

use super::{State, timestamp_ms};

/// The SQL of a JSON array of the pull requests of thread `$thread`, for `decode_list`.
macro_rules! pulls_json {
    ($thread:literal) => {
        concat!(
            "(SELECT json_group_array(json_array(repository, number, status))
              FROM (SELECT repository, number, status FROM thread_pulls
                    WHERE thread_pulls.thread_id = ",
            $thread,
            " ORDER BY added_at, rowid))"
        )
    };
}
pub(crate) use pulls_json;

/// One kept pull request of a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptPull {
    pub thread_id: String,
    pub number: u64,
    pub pull: Option<PullStatus>,
}

impl KeptPull {
    /// Merged or closed: read no more unless the forge lists it again.
    pub fn settled(&self) -> bool {
        self.pull
            .as_ref()
            .is_some_and(|pull| matches!(pull.state, PullState::Merged | PullState::Closed))
    }
}

/// A pull request as a chat keeps it: its checks counted, not listed, so the list of
/// chats stays small.
pub fn slim_pull(mut pull: PullStatus) -> PullStatus {
    pull.checks.list = Vec::new();
    pull
}

impl State {
    /// Looks through the messages of the chats started in `dir` that were not looked
    /// through yet for links to `repository`'s pull requests, whose pages start with
    /// `prefix`, and keeps each new one. Answers with the chats that got one.
    pub async fn find_pull_links(
        &self,
        dir: &str,
        repository: &str,
        prefix: &str,
    ) -> Result<Vec<String>> {
        let mut transaction = self.pool.begin().await.context("begin the link search")?;
        let chats: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT threads.id, threads.pulls_scanned,
                COALESCE((SELECT MAX(sequence) FROM messages
                          WHERE messages.thread_id = threads.id), 0)
             FROM threads JOIN thread_projects ON thread_projects.thread_id = threads.id
             WHERE thread_projects.dir = ? AND threads.parent_thread_id IS NULL",
        )
        .bind(dir)
        .fetch_all(&mut *transaction)
        .await
        .context("find the folder's chats")?;
        let lower = prefix.to_ascii_lowercase();
        let now = timestamp_ms()?;
        let mut linked = Vec::new();
        for (thread_id, scanned, newest) in chats {
            if newest <= scanned {
                continue;
            }
            // SQLite's lower() lowers ASCII only, as `pull_numbers` compares.
            let texts: Vec<String> = sqlx::query_scalar(
                "SELECT content FROM messages
                 WHERE thread_id = ? AND sequence > ? AND sequence <= ?
                    AND role IN ('user', 'assistant') AND instr(lower(content), ?) > 0
                 ORDER BY sequence",
            )
            .bind(&thread_id)
            .bind(scanned)
            .bind(newest)
            .bind(&lower)
            .fetch_all(&mut *transaction)
            .await
            .context("read the chat's new messages")?;
            let mut added = false;
            for number in texts.iter().flat_map(|text| pull_numbers(text, prefix)) {
                let number = i64::try_from(number).context("pull request number is too large")?;
                let rows = sqlx::query(
                    "INSERT OR IGNORE INTO thread_pulls
                        (thread_id, repository, number, status, added_at)
                     VALUES (?, ?, ?, NULL, ?)",
                )
                .bind(&thread_id)
                .bind(repository)
                .bind(number)
                .bind(now)
                .execute(&mut *transaction)
                .await
                .context("keep a chat's pull request")?
                .rows_affected();
                added |= rows == 1;
            }
            sqlx::query("UPDATE threads SET pulls_scanned = ? WHERE id = ?")
                .bind(newest)
                .bind(&thread_id)
                .execute(&mut *transaction)
                .await
                .context("mark the chat looked through")?;
            if added {
                linked.push(thread_id);
            }
        }
        transaction
            .commit()
            .await
            .context("commit the link search")?;
        Ok(linked)
    }

    /// Keeps `pull` as a pull request of the chat, as when it was made from the chat.
    /// True when the chat's list changed.
    pub async fn add_thread_pull(
        &self,
        thread_id: &str,
        repository: &str,
        pull: &PullStatus,
    ) -> Result<bool> {
        let status =
            serde_json::to_string(&slim_pull(pull.clone())).context("write the pull request")?;
        let rows = sqlx::query(
            "INSERT INTO thread_pulls (thread_id, repository, number, status, added_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (thread_id, repository, number) DO UPDATE SET status = excluded.status
             WHERE status IS NOT excluded.status",
        )
        .bind(thread_id)
        .bind(repository)
        .bind(i64::try_from(pull.number).context("pull request number is too large")?)
        .bind(status)
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("keep a chat's pull request")?
        .rows_affected();
        Ok(rows == 1)
    }

    /// The kept pull requests of `repository` in the chats started in `dir`.
    pub async fn folder_pulls(&self, dir: &str, repository: &str) -> Result<Vec<KeptPull>> {
        let rows = sqlx::query(
            "SELECT thread_pulls.thread_id, number, status
             FROM thread_pulls JOIN thread_projects
                ON thread_projects.thread_id = thread_pulls.thread_id
             WHERE thread_projects.dir = ? AND repository = ?",
        )
        .bind(dir)
        .bind(repository)
        .fetch_all(&self.pool)
        .await
        .context("read the folder's pull requests")?;
        rows.into_iter()
            .map(|row| {
                let number: i64 = row.try_get("number")?;
                Ok(KeptPull {
                    thread_id: row.try_get("thread_id")?,
                    number: u64::try_from(number).context("negative pull request number")?,
                    pull: decode(row.try_get("status")?),
                })
            })
            .collect()
    }

    /// Stores what the forge now says of pull request `number` of the chat.
    pub async fn set_thread_pull(
        &self,
        thread_id: &str,
        repository: &str,
        pull: &PullStatus,
    ) -> Result<()> {
        let status =
            serde_json::to_string(&slim_pull(pull.clone())).context("write the pull request")?;
        sqlx::query(
            "UPDATE thread_pulls SET status = ?
             WHERE thread_id = ? AND repository = ? AND number = ?",
        )
        .bind(status)
        .bind(thread_id)
        .bind(repository)
        .bind(i64::try_from(pull.number).context("pull request number is too large")?)
        .execute(&self.pool)
        .await
        .context("store a chat's pull request")?;
        Ok(())
    }

    /// The chat's pull requests, the first linked first.
    pub async fn thread_pulls(&self, thread_id: &str) -> Result<Vec<ThreadPull>> {
        let json: String = sqlx::query_scalar(concat!("SELECT ", pulls_json!("?"),))
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .context("read the chat's pull requests")?;
        Ok(decode_list(&json))
    }
}

/// A list `pulls_json` made. What the database holds is read leniently: a status of
/// another shape, as a newer build may write, counts as not read yet.
pub(crate) fn decode_list(json: &str) -> Vec<ThreadPull> {
    let Ok(rows) = serde_json::from_str::<Vec<(String, u64, Option<String>)>>(json) else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|(repository, number, status)| ThreadPull {
            repository,
            number,
            pull: decode(status),
        })
        .collect()
}

fn decode(status: Option<String>) -> Option<PullStatus> {
    serde_json::from_str(&status?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::thread::conversation::messages::{NewMessage, insert_message};
    use crate::testing::temp_dir;
    use dusk_protocol::project::Checks;

    const PREFIX: &str = "https://github.com/o/r/pull/";

    fn pull(number: u64, state: PullState) -> PullStatus {
        PullStatus {
            number,
            title: "Fix it".to_owned(),
            state,
            branch: "fix".to_owned(),
            author: String::new(),
            created: None,
            updated: None,
            added: 3,
            removed: 1,
            files: None,
            url: format!("{PREFIX}{number}"),
            checks_url: String::new(),
            checks: Checks::default(),
        }
    }

    #[tokio::test]
    async fn a_chat_keeps_the_pulls_it_linked_across_a_restart_and_a_delete() {
        let path = temp_dir("thread_pulls").join("state.db");
        let state = State::open(&path).await.unwrap();
        for (id, dir) in [("chat", "/p"), ("other", "/q")] {
            state.create_thread(id, "test").await.unwrap();
            state.set_thread_project(id, dir).await.unwrap();
        }
        state
            .create_agent_thread("agent", "chat", "general", "Aster-01")
            .await
            .unwrap();
        let say = async |thread: &str, message: NewMessage| {
            let mut transaction = state.pool.begin().await.unwrap();
            insert_message(&mut transaction, thread, &message, None, 1)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
        };
        say("chat", NewMessage::user(&format!("look at {PREFIX}7"))).await;
        let said = format!("Opened {PREFIX}12 and {PREFIX}7; not https://github.com/x/r/pull/3");
        say("chat", NewMessage::assistant(said, None, Vec::new())).await;
        // A system prompt and an agent's thread link nothing for the chat.
        say("chat", NewMessage::system(&format!("{PREFIX}30"))).await;
        say("agent", NewMessage::user(&format!("{PREFIX}40"))).await;
        say("other", NewMessage::user(&format!("{PREFIX}50"))).await;

        let linked = state.find_pull_links("/p", "o/r", PREFIX).await.unwrap();
        assert_eq!(linked, ["chat"]);
        assert!(
            state
                .find_pull_links("/p", "o/r", PREFIX)
                .await
                .unwrap()
                .is_empty()
        );
        let numbers = |pulls: Vec<ThreadPull>| pulls.iter().map(|p| p.number).collect::<Vec<_>>();
        assert_eq!(numbers(state.thread_pulls("chat").await.unwrap()), [7, 12]);

        // Only the messages after the last search are read again.
        say(
            "chat",
            NewMessage::user(&format!("and {PREFIX}12, {PREFIX}9")),
        )
        .await;
        assert_eq!(
            state.find_pull_links("/p", "o/r", PREFIX).await.unwrap(),
            ["chat"]
        );
        assert_eq!(
            numbers(state.thread_pulls("chat").await.unwrap()),
            [7, 12, 9]
        );

        let merged = pull(12, PullState::Merged);
        state.set_thread_pull("chat", "o/r", &merged).await.unwrap();
        // A row another build wrote in a shape this one does not read is not read yet.
        sqlx::query("UPDATE thread_pulls SET status = '{\"number\":\"x\"}' WHERE number = 9")
            .execute(&state.pool)
            .await
            .unwrap();
        state.close().await;

        let state = State::open(&path).await.unwrap();
        let kept = state.folder_pulls("/p", "o/r").await.unwrap();
        assert_eq!(kept.len(), 3);
        assert!(kept.iter().any(|k| k.number == 12 && k.settled()));
        assert!(kept.iter().any(|k| k.number == 9 && k.pull.is_none()));
        let listed = state.list_threads().await.unwrap();
        let chat = listed.iter().find(|thread| thread.id == "chat").unwrap();
        assert_eq!(numbers(chat.pulls.clone()), [7, 12, 9]);
        assert_eq!(chat.pulls[1].pull.as_ref(), Some(&merged));
        assert_eq!(chat.pulls[1].repository, "o/r");
        assert!(
            listed
                .iter()
                .find(|t| t.id == "other")
                .unwrap()
                .pulls
                .is_empty()
        );

        // Made from the chat: kept at once, and told once.
        let made = pull(60, PullState::Open);
        assert!(state.add_thread_pull("other", "o/r", &made).await.unwrap());
        assert!(!state.add_thread_pull("other", "o/r", &made).await.unwrap());

        state.delete_thread("chat").await.unwrap();
        assert!(state.folder_pulls("/p", "o/r").await.unwrap().is_empty());
        state.close().await;
    }
}
