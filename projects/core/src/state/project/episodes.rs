//! Episodes: what ended a problem in a project, kept so the same wall is not climbed
//! twice. This file only stores and finds them; the text arrives cleaned and cut to the
//! table's caps (`project::episodes`), and the table refuses anything longer.

use std::{collections::HashMap, sync::PoisonError};

use anyhow::{Context, Result, bail};
use sqlx::{Row, sqlite::SqliteRow};

use crate::state::{State, timestamp_ms};

/// Episodes kept per project; past it the least seen, oldest go.
const MAX_PER_PROJECT: i64 = 500;
/// Episodes `recall` answers with.
const RECALL_LIMIT: i64 = 3;
/// A page of `list_episodes`, at most.
const MAX_PAGE: u32 = 100;
/// How much of the model's query is read, in characters.
const QUERY_CHARS: usize = 200;
/// Words of the query searched for, at most.
const QUERY_WORDS: usize = 8;
/// The trigram index finds nothing shorter.
const MIN_WORD_CHARS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeKind {
    /// An error that came back ended with a call that passed.
    Fixed,
    /// A goal's verification passed.
    Verified,
}

impl EpisodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Verified => "verified",
        }
    }
}

/// An episode to store, its text already cleaned and within the table's caps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEpisode {
    /// A saved project's `dir`.
    pub project: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: EpisodeKind,
    /// SHA-256 in hex of what makes two episodes the same one.
    pub signature: String,
    pub tool: String,
    pub model: String,
    pub problem: String,
    pub failed: String,
    pub worked: String,
    /// Empty when the model gave none.
    pub lesson: String,
}

/// A stored episode, without its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub id: i64,
    pub project: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: EpisodeKind,
    pub tool: String,
    pub model: String,
    pub problem: String,
    pub failed: String,
    pub worked: String,
    pub lesson: String,
    pub seen: u64,
    pub created_at: i64,
    pub seen_at: i64,
}

/// What a `recall` query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recall {
    /// Best first; empty when nothing matched.
    Found(Vec<Episode>),
    /// The query has no word of three characters or more, so there is nothing to search.
    NoUsableWord,
}

/// The episodes `$filter` picks, every column but the signature.
macro_rules! select {
    ($from:literal, $filter:literal) => {
        concat!(
            "SELECT e.id, e.project, e.thread_id, e.turn_id, e.kind, e.tool, e.model,
                e.problem, e.failed, e.worked, e.lesson, e.seen, e.created_at, e.seen_at
             FROM ",
            $from,
            " ",
            $filter
        )
    };
}

// The callers come with tasks E2 (recording), E3 (the lesson), E4 (`recall`) and E5
// (listing and removing for clients); until they land nothing calls these.
#[allow(dead_code)]
impl State {
    /// Stores an episode, or counts it again when the project has one of its signature:
    /// then the newer `worked` and links are taken and a lesson is kept unless a new one
    /// came. Past `MAX_PER_PROJECT` the least seen, oldest others are removed in the same
    /// transaction. Answers with the episode's id.
    pub async fn record_episode(&self, episode: &NewEpisode) -> Result<i64> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin the episode")?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO episodes (project, thread_id, turn_id, kind, signature, tool, model,
                problem, failed, worked, lesson, created_at, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
             ON CONFLICT (project, signature) DO UPDATE SET
                seen = seen + 1,
                seen_at = max(seen_at, excluded.seen_at),
                worked = excluded.worked,
                lesson = CASE WHEN excluded.lesson != '' THEN excluded.lesson ELSE lesson END,
                thread_id = excluded.thread_id,
                turn_id = excluded.turn_id
             RETURNING id",
        )
        .bind(&episode.project)
        .bind(&episode.thread_id)
        .bind(&episode.turn_id)
        .bind(episode.kind.as_str())
        .bind(&episode.signature)
        .bind(&episode.tool)
        .bind(&episode.model)
        .bind(&episode.problem)
        .bind(&episode.failed)
        .bind(&episode.worked)
        .bind(&episode.lesson)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await
        .context("save the episode; is the project saved and the text within its caps?")?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE project = ?")
            .bind(&episode.project)
            .fetch_one(&mut *transaction)
            .await
            .context("count the project's episodes")?;
        let over = count - MAX_PER_PROJECT;
        if over > 0 {
            // The episode just written is never the one dropped: it would always be the
            // least seen, and a full project would then learn nothing new.
            sqlx::query(
                "DELETE FROM episodes WHERE id IN (
                    SELECT id FROM episodes WHERE project = ?1 AND id != ?2
                    ORDER BY seen, seen_at, id LIMIT ?3)",
            )
            .bind(&episode.project)
            .bind(id)
            .bind(over)
            .execute(&mut *transaction)
            .await
            .context("remove the least seen episodes")?;
        }
        transaction.commit().await.context("commit the episode")?;
        Ok(id)
    }

    /// Puts the model's lesson on the episodes the turn recorded; an empty one changes
    /// nothing. Answers with how many it was put on.
    pub async fn set_turn_lesson(&self, turn_id: &str, lesson: &str) -> Result<u64> {
        if lesson.is_empty() {
            return Ok(0);
        }
        let done = sqlx::query("UPDATE episodes SET lesson = ? WHERE turn_id = ?")
            .bind(lesson)
            .bind(turn_id)
            .execute(&self.pool)
            .await
            .context("save the lesson; is it within its cap?")?;
        Ok(done.rows_affected())
    }

    /// The project's episodes that best match the model's `query`. Its text is never
    /// read as query syntax: only its words are searched, each as a quoted phrase.
    pub async fn recall_episodes(&self, project: &str, query: &str) -> Result<Recall> {
        let Some(phrases) = match_phrases(query) else {
            return Ok(Recall::NoUsableWord);
        };
        let found = sqlx::query(select!(
            "episodes_search s JOIN episodes e ON e.id = s.rowid",
            "WHERE episodes_search MATCH ?2 AND e.project = ?1
             ORDER BY bm25(episodes_search, 4.0, 1.0, 2.0, 2.0), e.seen DESC, e.seen_at DESC
             LIMIT ?3"
        ))
        .bind(project)
        .bind(phrases)
        .bind(RECALL_LIMIT)
        .try_map(decode)
        .fetch_all(&self.pool)
        .await
        .context("search the episodes")?;
        Ok(Recall::Found(found))
    }

    /// The project's episodes, newest first, `limit` (at most `MAX_PAGE`) of those
    /// older than the episode `after`.
    pub async fn list_episodes(
        &self,
        project: &str,
        after: Option<i64>,
        limit: u32,
    ) -> Result<Vec<Episode>> {
        // Ids only grow, so the newest has the largest.
        sqlx::query(select!(
            "episodes e",
            "WHERE e.project = ?1 AND (?2 IS NULL OR e.id < ?2) ORDER BY e.id DESC LIMIT ?3"
        ))
        .bind(project)
        .bind(after)
        .bind(limit.min(MAX_PAGE))
        .try_map(decode)
        .fetch_all(&self.pool)
        .await
        .context("list the episodes")
    }

    /// Removes one of the project's episodes; false when the project has no such one.
    pub async fn remove_episode(&self, project: &str, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM episodes WHERE project = ? AND id = ?")
            .bind(project)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("remove the episode")?;
        let removed = done.rows_affected() > 0;
        if removed {
            // `recall` is offered while a project has one; the last may have gone.
            self.load_episode_counts().await?;
        }
        Ok(removed)
    }

    /// How many episodes each project has, by `dir`; projects with none are left out.
    pub async fn episode_counts(&self) -> Result<HashMap<String, u64>> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT project, COUNT(*) FROM episodes GROUP BY project")
                .fetch_all(&self.pool)
                .await
                .context("count the episodes")?;
        rows.into_iter()
            .map(|(project, count)| Ok((project, u64::try_from(count)?)))
            .collect()
    }

    /// Reads the counts from the table into memory; the database does it when it opens.
    pub(crate) async fn load_episode_counts(&self) -> Result<()> {
        let counts = self.episode_counts().await?;
        *self
            .episodes
            .write()
            .unwrap_or_else(PoisonError::into_inner) = counts;
        Ok(())
    }

    /// Whether the project has an episode, from memory, with no query.
    pub fn has_episodes(&self, project: &str) -> bool {
        let counts = self.episodes.read().unwrap_or_else(PoisonError::into_inner);
        counts.get(project).is_some_and(|count| *count > 0)
    }

    /// Tells `recall` that the project has one more episode; whoever records one calls it
    /// after the write. It adds one even when the write only counted a known episode
    /// again: the number only has to tell none from some.
    pub fn note_episode(&self, project: &str) {
        let mut counts = self
            .episodes
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *counts.entry(project.to_owned()).or_default() += 1;
    }
}

/// The FTS5 query for the model's text: up to `QUERY_WORDS` words of `MIN_WORD_CHARS`
/// or more from its first `QUERY_CHARS` characters, each a quoted phrase, any of them
/// enough. Punctuation around a word is dropped, so `"cargo` and `problem:` search for
/// the word; inside a phrase a quote is doubled, so no text leaves its quotes.
fn match_phrases(query: &str) -> Option<String> {
    let head: String = query.chars().take(QUERY_CHARS).collect();
    let phrases: Vec<String> = head
        .split(|c: char| c.is_whitespace() || c.is_control())
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|word| word.chars().count() >= MIN_WORD_CHARS)
        .take(QUERY_WORDS)
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect();
    (!phrases.is_empty()).then(|| phrases.join(" OR "))
}

fn decode(row: SqliteRow) -> sqlx::Result<Episode> {
    from_row(&row).map_err(|error| sqlx::Error::Decode(error.into()))
}

fn from_row(row: &SqliteRow) -> Result<Episode> {
    let kind = match row.try_get::<&str, _>("kind")? {
        "fixed" => EpisodeKind::Fixed,
        "verified" => EpisodeKind::Verified,
        other => bail!("unknown episode kind {other:?}"),
    };
    Ok(Episode {
        id: row.try_get("id")?,
        project: row.try_get("project")?,
        thread_id: row.try_get("thread_id")?,
        turn_id: row.try_get("turn_id")?,
        kind,
        tool: row.try_get("tool")?,
        model: row.try_get("model")?,
        problem: row.try_get("problem")?,
        failed: row.try_get("failed")?,
        worked: row.try_get("worked")?,
        lesson: row.try_get("lesson")?,
        seen: u64::try_from(row.try_get::<i64, _>("seen")?).context("negative seen count")?,
        created_at: row.try_get("created_at")?,
        seen_at: row.try_get("seen_at")?,
    })
}

#[cfg(test)]
#[path = "episodes_tests.rs"]
mod tests;
