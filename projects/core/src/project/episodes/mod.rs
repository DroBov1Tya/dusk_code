//! Episodes: what ended a problem in a project, written down from what the core saw so
//! the same wall is not climbed twice (`docs/project-intelligence.md`, section 6). This
//! is the way in to the store: it builds an episode's fields from the facts of a turn,
//! cleans every text (`clean`), and refuses what must not be kept. Finding the facts
//! (which loop ended, where the lesson line is) is the runtime's.
//!
//! An episode is refused, without an error, when the folder is not a saved project's,
//! when a command of it names a file whose text is never kept (`ledger::may_keep_text`),
//! or when a text it needs is empty once cleaned.

mod clean;
#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::Result;

pub use self::clean::bounded;
use self::clean::{Field, Secrets, clean, scrub};
use super::ledger::{may_keep_text, sha256};
use crate::state::{EpisodeKind, NewEpisode, State};

/// Changed files an episode names, at most.
const MAX_CHANGED_PATHS: usize = 4;
/// Between the attempts of `failed` and between the commands of a check.
const BETWEEN: &str = " | ";
/// What a command line is cut at to find the paths it names.
const COMMAND_BREAKS: &[char] = &[
    '\'', '"', '`', '=', ':', ';', ',', '|', '&', '<', '>', '(', ')', '[', ']', '{', '}', '\\',
];

/// Where and by whom the episode happened.
#[derive(Debug, Clone, Copy)]
pub struct Origin<'a> {
    /// The thread's folder, resolved as the ledger spells it.
    pub project: &'a Path,
    pub thread_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub tool: &'a str,
    pub model: &'a str,
}

/// What happened, as the runtime saw it and before any cleaning.
#[derive(Debug, Clone, Copy)]
pub enum Facts<'a> {
    /// An error that came back ended with a call that passed.
    Fixed {
        /// The failure signature's key: what makes two such errors the same one.
        key: &'a str,
        /// The error lines, whole: a text cut before it is cleaned may keep half a key
        /// the rules no longer see.
        problem: &'a str,
        /// The calls that failed, oldest first, whole.
        attempts: &'a [String],
        /// The call that passed, whole.
        passed: &'a str,
        /// Files changed between the first failure and the pass, relative paths.
        changed: &'a [String],
        /// The model's line, when it gave one.
        lesson: Option<&'a str>,
    },
    /// A goal's verification passed.
    Verified {
        objective: &'a str,
        /// The commands of its checks that exited 0.
        commands: &'a [String],
    },
}

/// Cleans and stores an episode; `None` when it is refused. An error is logged here and
/// is the caller's to drop: an episode never fails a turn.
pub async fn record(state: &State, origin: &Origin<'_>, facts: &Facts<'_>) -> Result<Option<i64>> {
    let stored = store(state, origin, facts).await;
    if let Err(error) = &stored {
        // The error is the database's and names no text of the episode.
        tracing::warn!(kind = kind(facts), "cannot record an episode: {error:#}");
    }
    stored
}

/// Cleans the model's lesson and puts it on the episodes the turn recorded. Answers
/// with how many it was put on; none when the folder is not a saved project's.
pub async fn set_lesson(state: &State, project: &Path, turn_id: &str, lesson: &str) -> Result<u64> {
    let stored = store_lesson(state, project, turn_id, lesson).await;
    if let Err(error) = &stored {
        tracing::warn!("cannot record an episode's lesson: {error:#}");
    }
    stored
}

async fn store(state: &State, origin: &Origin<'_>, facts: &Facts<'_>) -> Result<Option<i64>> {
    if !commands(facts).all(names_no_secret_file) {
        return Ok(None);
    }
    let Some((project, secrets)) = saved_project(state, origin.project).await? else {
        return Ok(None);
    };
    let (kind, key, problem, failed, worked, lesson) = match *facts {
        Facts::Fixed {
            key,
            problem,
            attempts,
            passed,
            changed,
            lesson,
        } => (
            EpisodeKind::Fixed,
            scrub(key, &secrets),
            problem,
            other_attempts(attempts, passed),
            passed_after(passed, changed),
            lesson.unwrap_or_default(),
        ),
        Facts::Verified {
            objective,
            commands,
        } => (
            EpisodeKind::Verified,
            sorted(commands, &secrets),
            objective,
            String::new(),
            commands.join(BETWEEN),
            "",
        ),
    };
    let episode = NewEpisode {
        project,
        thread_id: origin.thread_id.map(str::to_owned),
        turn_id: origin.turn_id.map(str::to_owned),
        kind,
        signature: sha256(format!("{}\n{key}", self::kind(facts)).as_bytes()),
        tool: clean(origin.tool, &secrets, Field::Tool),
        model: clean(origin.model, &secrets, Field::Model),
        problem: clean(problem, &secrets, Field::Problem),
        failed: clean(&failed, &secrets, Field::Failed),
        worked: clean(&worked, &secrets, Field::Worked),
        lesson: clean(lesson, &secrets, Field::Lesson),
    };
    let needed = [
        &episode.tool,
        &episode.model,
        &episode.problem,
        &episode.worked,
    ];
    if needed.iter().any(|text| text.is_empty()) {
        return Ok(None);
    }
    state.record_episode(&episode).await.map(Some)
}

async fn store_lesson(state: &State, project: &Path, turn_id: &str, lesson: &str) -> Result<u64> {
    let Some((_, secrets)) = saved_project(state, project).await? else {
        return Ok(0);
    };
    let lesson = clean(lesson, &secrets, Field::Lesson);
    state.set_turn_lesson(turn_id, &lesson).await
}

/// The saved project whose folder is exactly `dir`, as the store names it, and the
/// values of its variables. A subfolder of a project is not the project.
async fn saved_project(state: &State, dir: &Path) -> Result<Option<(String, Secrets)>> {
    let Some(name) = saved_dir(state, dir).await? else {
        return Ok(None);
    };
    let variables = state.project_env_for_dir(&name).await?;
    let secrets = Secrets::new(variables.into_iter().map(|(_, value)| value));
    Ok(Some((name, secrets)))
}

/// The store's name for `dir` when it is exactly a saved project's folder; what episodes
/// are filed under, and what `recall` reads.
pub(crate) async fn saved_dir(state: &State, dir: &Path) -> Result<Option<String>> {
    let Some(name) = dir.to_str() else {
        return Ok(None);
    };
    let projects = state.list_projects().await?;
    Ok(projects
        .iter()
        .any(|project| project.dir == dir)
        .then(|| name.to_owned()))
}

fn kind(facts: &Facts<'_>) -> &'static str {
    match facts {
        Facts::Fixed { .. } => "fixed",
        Facts::Verified { .. } => "verified",
    }
}

/// Every command line the episode would keep.
fn commands<'a>(facts: &Facts<'a>) -> impl Iterator<Item = &'a str> {
    let (one, many) = match *facts {
        Facts::Fixed {
            attempts, passed, ..
        } => (Some(passed), attempts),
        Facts::Verified { commands, .. } => (None, commands),
    };
    one.into_iter().chain(many.iter().map(String::as_str))
}

/// Whether no piece of `command` is a path whose text is never kept. A command line is
/// not parsed as a shell would: it is cut at whitespace and at `COMMAND_BREAKS`, and
/// each piece is read as a path. So `cat .env`, `--env-file=.env`, `scp h:.ssh/id .` and
/// `"$HOME/.ssh/config"` are refused; a name a glob or a variable hides is not seen.
fn names_no_secret_file(command: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || c.is_control() || COMMAND_BREAKS.contains(&c))
        .all(may_keep_text)
}

/// The attempts that are not the call that passed, each once, joined.
fn other_attempts(attempts: &[String], passed: &str) -> String {
    let mut others: Vec<&str> = Vec::new();
    for attempt in attempts.iter().map(|attempt| attempt.trim()) {
        if !attempt.is_empty() && attempt != passed.trim() && !others.contains(&attempt) {
            others.push(attempt);
        }
    }
    others.join(BETWEEN)
}

/// The call that passed, and the files changed before it did. A file whose text is
/// never kept is not named either.
fn passed_after(passed: &str, changed: &[String]) -> String {
    let paths: Vec<&str> = changed
        .iter()
        .map(String::as_str)
        .filter(|path| !path.trim().is_empty() && may_keep_text(path))
        .take(MAX_CHANGED_PATHS)
        .collect();
    if paths.is_empty() {
        return passed.to_owned();
    }
    format!("{passed} after changes to {}", paths.join(", "))
}

/// The cleaned commands in one order, so the same checks are the same episode however
/// they were listed.
fn sorted(commands: &[String], secrets: &Secrets) -> String {
    let mut commands: Vec<String> = commands
        .iter()
        .map(|command| scrub(command, secrets))
        .collect();
    commands.sort();
    commands.dedup();
    commands.join("\n")
}
