//! An agent's own branch, from start to merge: each step through `project::git`, never a
//! shell. A thread touches only the branches it made, for itself or for an agent it
//! asked, and merges one only into the branch it was made from. Nothing is forced, no
//! branch with unmerged work is deleted, and the user's folder is never left mid-merge.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{ChangeKind, Project, ThreadBranch};

use crate::{
    project::git,
    runtime::thread::start::display_name,
    state::{State, ThreadWorktree},
    tools::{ToolContext, ToolScope, tool::printable},
};

/// The longest list of files a result names.
const MAX_LISTED: usize = 20;
/// What uncommitted work is saved as when its worktree is abandoned.
const LEFT_BEHIND: &str = "Work left uncommitted when the branch was abandoned";

/// What git needs of a folder; the rest of a project does not matter to it.
pub(super) fn folder(dir: &Path) -> Project {
    Project {
        id: String::new(),
        name: String::new(),
        description: String::new(),
        dir: dir.to_owned(),
        created_at: 0,
    }
}

pub(super) fn shown(worktree: &ThreadWorktree) -> ThreadBranch {
    ThreadBranch {
        branch: worktree.branch.clone(),
        base: worktree.base.clone(),
        dir: PathBuf::from(&worktree.dir),
    }
}

/// Makes a branch from the one checked out where `from` works, a worktree for it beside
/// the repository, and the record that moves `thread_id` there. `made_by` is the thread
/// that asked: `thread_id` itself, or the thread that asked it as an agent.
pub async fn start(
    state: &State,
    thread_id: &str,
    made_by: &str,
    from: &ToolScope,
    name: Option<&str>,
) -> Result<ThreadWorktree> {
    if let Some(open) = state.thread_worktree(thread_id).await? {
        bail!(
            "this thread works on branch {} in {} already: merge or abandon it first",
            open.branch,
            open.dir
        );
    }
    let repository = git::repository(from.cwd())
        .await
        .context("the folder is not in a git repository, so it cannot branch")?;
    let base = repository.branch.clone().context(
        "the folder has no branch checked out (a detached HEAD): nothing to merge back into",
    )?;
    let taken = git::names(from.cwd()).await?;
    let branch = free_name(name, thread_id, &taken)?;
    git::add_worktree(&folder(from.cwd()), &branch, Some(&base)).await?;
    let made = git::info(&folder(from.cwd()))
        .await?
        .worktrees
        .into_iter()
        .find(|tree| tree.branch.as_deref() == Some(branch.as_str()))
        .context("git made no worktree")?;
    let top = made
        .path
        .canonicalize()
        .context("the new worktree is not there")?;
    // A project inside a repository keeps its place inside the new checkout.
    let inner = top.join(&repository.prefix);
    let dir = match inner.is_dir() {
        true => inner.canonicalize()?,
        false => top.clone(),
    };
    let worktree = ThreadWorktree {
        thread_id: thread_id.to_owned(),
        dir: text(&dir)?,
        worktree_of: text(from.cwd())?,
        project: text(from.project_dir())?,
        branch,
        base,
        made_by: made_by.to_owned(),
    };
    if let Err(error) = state.add_thread_worktree(&worktree).await {
        // Nothing is in the new checkout yet, so it goes again without a trace.
        let _ = git::remove_worktree(&folder(from.cwd()), &top).await;
        let _ = git::remove_merged(from.cwd(), &worktree.branch).await;
        return Err(error);
    }
    Ok(worktree)
}

/// `agent/<name>`, the name cut to plain characters, or the thread's own when none
/// is given; a number is added while git has the name already.
fn free_name(name: Option<&str>, thread_id: &str, taken: &[String]) -> Result<String> {
    let wanted = name.unwrap_or(thread_id);
    let mut slug: String = wanted
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .collect();
    slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if name.is_none() {
        // A thread's ID begins with its time; its end tells threads apart.
        let start = slug.len().saturating_sub(8);
        slug = slug[start..].trim_start_matches('-').to_owned();
    }
    slug.truncate(40);
    let slug = slug.trim_end_matches('-');
    ensure!(!slug.is_empty(), "the branch name needs letters or digits");
    for number in 1..100 {
        let branch = match number {
            1 => format!("agent/{slug}"),
            _ => format!("agent/{slug}-{number}"),
        };
        git::check_branch(&branch)?;
        if !taken.contains(&branch) {
            return Ok(branch);
        }
    }
    bail!("every name for agent/{slug} is taken; give another name")
}

/// The branch `agent` works on, or this thread's own when `None`. An agent's counts only
/// when this thread made it.
pub async fn find(state: &State, thread_id: &str, agent: Option<&str>) -> Result<ThreadWorktree> {
    let Some(agent) = agent else {
        return state
            .thread_worktree(thread_id)
            .await?
            .context("this thread works on no branch of its own: `start` one first");
    };
    let asked = state.agent_threads(thread_id).await?;
    let thread = asked
        .iter()
        .find(|thread| thread.id == agent || display_name(thread).eq_ignore_ascii_case(agent))
        .with_context(|| format!("no agent {agent} was asked from here"))?;
    let worktree = state
        .thread_worktree(&thread.id)
        .await?
        .with_context(|| format!("{agent} works on no branch of its own"))?;
    ensure!(
        worktree.made_by == thread_id,
        "{agent}'s branch was not made here"
    );
    Ok(worktree)
}

/// Where the branch stands: commits ahead and behind, files not yet committed, and a
/// merge waiting with its conflicts.
pub async fn status(worktree: &ThreadWorktree) -> Result<String> {
    usable(worktree).await?;
    let dir = Path::new(&worktree.dir);
    let (ahead, behind) = git::apart(dir, &worktree.base, &worktree.branch).await?;
    let unsaved = git::unsaved(dir, true).await?;
    let mut lines = vec![format!(
        "Branch {} from {}, in {}: {ahead} commit{} ahead of {}, {behind} behind.",
        worktree.branch,
        worktree.base,
        worktree.dir,
        plural(ahead),
        worktree.base
    )];
    lines.push(match unsaved.is_empty() {
        true => "Nothing uncommitted.".to_owned(),
        false => format!("Uncommitted: {}.", listed(&unsaved)),
    });
    if git::merging(dir).await {
        let conflicts = conflicts(dir).await?;
        lines.push(match conflicts.is_empty() {
            true => "A merge waits: `commit` finishes it, `abort` drops it.".to_owned(),
            false => format!(
                "A merge waits with conflicts in {}: edit them, then `commit`, or `abort`.",
                listed(&conflicts)
            ),
        });
    }
    Ok(printable(&lines.join("\n")))
}

/// Commits every change in the worktree, new files too; finishes a merge whose conflicts
/// are resolved.
pub async fn commit(worktree: &ThreadWorktree, message: &str) -> Result<String> {
    let dir = Path::new(&worktree.dir);
    let top = top(dir).await?;
    // A conflicted file counts as resolved once its markers are gone; staging marks it so.
    let marked: Vec<String> = conflicts(dir)
        .await?
        .into_iter()
        .filter(|path| has_markers(&top.join(path)))
        .collect();
    ensure!(
        marked.is_empty(),
        "conflict markers are left in {}: edit them first",
        listed(&marked)
    );
    let merging = git::merging(dir).await;
    if git::unsaved(dir, true).await?.is_empty() && !merging {
        return Ok("Nothing to commit: the worktree is clean.".into());
    }
    git::stage_all(&folder(&top)).await?;
    let outcome = git::commit(&folder(&top), message).await?;
    let (ahead, _) = git::apart(dir, &worktree.base, &worktree.branch).await?;
    Ok(printable(&format!(
        "Committed on {}; it is {ahead} commit{} ahead of {}. {}",
        worktree.branch,
        plural(ahead),
        worktree.base,
        outcome.message
    )))
}

/// Aborts the merge waiting in the worktree.
pub async fn abort(worktree: &ThreadWorktree) -> Result<String> {
    let dir = Path::new(&worktree.dir);
    ensure!(git::merging(dir).await, "no merge waits in the worktree");
    git::abort_merge(&folder(&top(dir).await?)).await?;
    Ok(format!(
        "The merge is aborted; {} is as it was before it.",
        worktree.branch
    ))
}

/// What a merge or an abandon did.
pub enum Done {
    /// The worktree and its record are gone; the thread works in its folder again.
    Finished(String),
    /// The worktree and its record stay: the branch needs the agent first, such as
    /// conflicts to resolve, or a merge is done and its worktree could not be removed.
    Waits(String),
}

/// Why the branch cannot be worked on any more, when something it needs is gone: its
/// worktree, the folder it was made from, or the branch it merges into. Such a record
/// only `abandon` clears.
async fn lost(worktree: &ThreadWorktree) -> Option<String> {
    let dir = Path::new(&worktree.dir);
    let origin = Path::new(&worktree.worktree_of);
    if !dir.is_dir() || git::repository(dir).await.is_none() {
        return Some(format!("the worktree {} is gone", worktree.dir));
    }
    if git::repository(origin).await.is_none() {
        return Some(format!(
            "{} it was made from is no longer a repository",
            worktree.worktree_of
        ));
    }
    match git::names(origin).await {
        Ok(names) if names.contains(&worktree.base) => None,
        Ok(_) => Some(format!(
            "the branch {} it was made from is gone",
            worktree.base
        )),
        Err(error) => Some(format!("its branches cannot be read: {error:#}")),
    }
}

/// Fails with what is gone and the way out, when something is.
async fn usable(worktree: &ThreadWorktree) -> Result<()> {
    if let Some(reason) = lost(worktree).await {
        bail!(
            "{reason}: `abandon` forgets this record; the branch {} keeps its commits",
            worktree.branch
        );
    }
    Ok(())
}

/// Refused while an agent's branch made from this one is open: merging or removing this
/// one first would leave that agent nothing to merge into.
async fn no_agents_on(state: &State, worktree: &ThreadWorktree) -> Result<()> {
    let open: Vec<String> = state
        .worktrees_made_by(&worktree.thread_id)
        .await?
        .into_iter()
        .filter(|other| other.thread_id != worktree.thread_id && other.base == worktree.branch)
        .map(|other| other.branch)
        .collect();
    ensure!(
        open.is_empty(),
        "agents' branches made from {} are open ({}): merge or abandon them first",
        worktree.branch,
        listed(&open)
    );
    Ok(())
}

/// Refused unless the folder the merge goes into is on `base` with no tracked changes:
/// a merge there must neither land on another branch nor mix in the user's work.
async fn ready_for_merge(origin: &Path, base: &str) -> Result<()> {
    let there = git::repository(origin)
        .await
        .with_context(|| format!("{} is no longer a repository", origin.display()))?;
    ensure!(
        there.branch.as_deref() == Some(base),
        "{} is on {} now, not {}: merging there would change another branch; ask the user",
        origin.display(),
        there.branch.as_deref().unwrap_or("a detached HEAD"),
        base
    );
    let theirs = git::unsaved(origin, false).await?;
    ensure!(
        theirs.is_empty(),
        "{} has uncommitted changes in tracked files ({}): ask the user to commit or stash \
         them before the merge",
        origin.display(),
        listed(&theirs)
    );
    Ok(())
}

/// The error of a merge into `origin` that did not finish, with whether the folder is
/// left mid-merge: one is aborted when it can be, and said plainly when it cannot.
async fn merge_failed(origin: &Path, error: anyhow::Error) -> anyhow::Error {
    if !git::merging(origin).await {
        return error.context(format!(
            "the merge failed; {} is not mid-merge",
            origin.display()
        ));
    }
    match git::abort_merge(&folder(origin)).await {
        Ok(_) => error.context(format!(
            "the merge failed and was aborted; {} is as it was before",
            origin.display()
        )),
        Err(abort) => error.context(format!(
            "the merge failed and could not be aborted ({abort:#}): {} is left mid-merge; \
             ask the user to finish or abort it there",
            origin.display()
        )),
    }
}

/// Merges the branch into the one it was made from, as a merge commit. The base comes
/// into the branch first, in the worktree, so conflicts are resolved there and never
/// in the user's folder. The user says yes to a merge into a branch no agent made.
/// Then the worktree goes, and the branch with it, since its work is merged.
pub async fn merge(
    context: &ToolContext,
    state: &State,
    worktree: &ThreadWorktree,
) -> Result<Done> {
    usable(worktree).await?;
    no_agents_on(state, worktree).await?;
    let dir = Path::new(&worktree.dir);
    let origin = Path::new(&worktree.worktree_of);
    ensure!(
        !git::merging(dir).await,
        "a merge waits in the worktree: resolve its conflicts and `commit`, or `abort`"
    );
    let unsaved = git::unsaved(dir, true).await?;
    ensure!(
        unsaved.is_empty(),
        "commit the work first; uncommitted: {}",
        listed(&unsaved)
    );
    ready_for_merge(origin, &worktree.base).await?;
    let (ahead, behind) = git::apart(dir, &worktree.base, &worktree.branch).await?;
    if ahead == 0 {
        let note = finish(state, worktree).await?;
        return Ok(Done::Finished(format!(
            "{} has no commits that {} lacks: nothing to merge. {note}",
            worktree.branch, worktree.base
        )));
    }
    if behind > 0 {
        let outcome = git::merge(&folder(&top(dir).await?), &worktree.base).await?;
        if !outcome.conflicts.is_empty() {
            return Ok(Done::Waits(printable(&format!(
                "{} moved on by {behind} commit{}. Bringing it into {} left conflicts in {} \
                 (in {}). Resolve them there, `commit`, check the work again and `merge`; or \
                 `abort`.",
                worktree.base,
                plural(behind),
                worktree.branch,
                listed(&outcome.conflicts),
                worktree.dir
            ))));
        }
    }
    let files = git::files_between(dir, &worktree.base, &worktree.branch).await?;
    let action = format!(
        "merge branch {} ({ahead} commit{}, {} file{}: {}) into {} in {}",
        worktree.branch,
        plural(ahead),
        files.len(),
        plural(files.len() as u64),
        listed(&files),
        worktree.base,
        origin.display()
    );
    // A branch is an agent's when a thread works on it there; any other is the user's.
    let agents = state
        .worktree_at(&worktree.worktree_of)
        .await?
        .is_some_and(|parent| parent.branch == worktree.base);
    match agents {
        true => context.approve_git(&action).await?,
        false => context.ask_always(&action).await?,
    }
    // The answer can take long, and the user may switch or edit their folder meanwhile:
    // the yes was for a merge into `base` as it was, so that is checked again.
    ready_for_merge(origin, &worktree.base).await?;
    let outcome = match git::merge_recorded(&folder(origin), &worktree.branch).await {
        Ok(outcome) => outcome,
        Err(error) => return Err(merge_failed(origin, error).await),
    };
    if !outcome.conflicts.is_empty() {
        let moved = anyhow::anyhow!(
            "{} moved on during the merge and it conflicted in {}: `merge` again",
            worktree.base,
            listed(&outcome.conflicts)
        );
        return Err(merge_failed(origin, moved).await);
    }
    let merged = format!(
        "Merged {} into {} in {}.",
        worktree.branch,
        worktree.base,
        origin.display()
    );
    // The merge commit is there; a worktree that cannot go is a note, not a failure.
    Ok(match finish(state, worktree).await {
        Ok(note) => Done::Finished(printable(&format!("{merged} {note}"))),
        Err(error) => Done::Waits(printable(&format!(
            "{merged} The worktree {} could not be removed ({error:#}); `abandon` it once \
             what is left there is dealt with.",
            worktree.dir
        ))),
    })
}

/// Removes the worktree. Uncommitted work is committed to the branch first, and a branch
/// with work on it is kept, so nothing is lost; the user says yes to leaving such work
/// behind, every time.
pub async fn abandon(
    context: &ToolContext,
    state: &State,
    worktree: &ThreadWorktree,
) -> Result<Done> {
    if let Some(reason) = lost(worktree).await {
        return forget(context, state, worktree, &reason).await;
    }
    no_agents_on(state, worktree).await?;
    let dir = Path::new(&worktree.dir);
    ensure!(
        !git::merging(dir).await,
        "a merge waits in the worktree: `abort` it first"
    );
    let unsaved = git::unsaved(dir, true).await?;
    let (ahead, _) = git::apart(dir, &worktree.base, &worktree.branch).await?;
    if unsaved.is_empty() && ahead == 0 {
        context
            .approve_git(&format!(
                "remove the worktree of {} in {}, which has no work",
                worktree.branch, worktree.dir
            ))
            .await?;
    } else {
        let mut kept = Vec::new();
        if !unsaved.is_empty() {
            kept.push(format!(
                "{} uncommitted file{} ({}) committed to it",
                unsaved.len(),
                plural(unsaved.len() as u64),
                listed(&unsaved)
            ));
        }
        if ahead > 0 {
            kept.push(format!("{ahead} commit{} not merged", plural(ahead)));
        }
        context
            .ask_always(&format!(
                "abandon branch {} without merging it into {}: the worktree {} is removed; \
                 the branch keeps its work ({})",
                worktree.branch,
                worktree.base,
                worktree.dir,
                kept.join(", ")
            ))
            .await?;
        if !unsaved.is_empty() {
            let top = top(dir).await?;
            git::stage_all(&folder(&top)).await?;
            git::commit(&folder(&top), LEFT_BEHIND).await?;
        }
    }
    let note = finish(state, worktree).await?;
    Ok(Done::Finished(format!(
        "Abandoned {}. {note}",
        worktree.branch
    )))
}

/// Drops the record of a branch whose worktree, origin or base is gone. Nothing in git
/// is touched: the branch keeps its commits, and a worktree folder still there stays.
async fn forget(
    context: &ToolContext,
    state: &State,
    worktree: &ThreadWorktree,
    reason: &str,
) -> Result<Done> {
    context
        .approve_git(&format!(
            "forget branch {}, since {reason}; git is not touched",
            worktree.branch
        ))
        .await?;
    state.remove_thread_worktree(&worktree.thread_id).await?;
    let folder = match Path::new(&worktree.dir).exists() {
        true => format!(" The folder {} is left as it is.", worktree.dir),
        false => String::new(),
    };
    Ok(Done::Finished(printable(&format!(
        "Forgot {}: {reason}. The branch is kept with its commits, if git still has it; \
         the thread works in its own folder again.{folder}",
        worktree.branch
    ))))
}

/// Every branch this thread and the agents it asked work on, with where each stands.
pub async fn list(state: &State, thread_id: &str) -> Result<String> {
    let mut worktrees: Vec<ThreadWorktree> = state
        .thread_worktree(thread_id)
        .await?
        .into_iter()
        .collect();
    worktrees.extend(
        state
            .worktrees_made_by(thread_id)
            .await?
            .into_iter()
            .filter(|worktree| worktree.thread_id != thread_id),
    );
    if worktrees.is_empty() {
        return Ok("No branches: this thread and its agents work in their own folders.".into());
    }
    let mut lines = Vec::new();
    for worktree in &worktrees {
        let who = match worktree.thread_id == thread_id {
            true => "this thread".to_owned(),
            false => state.load_thread(&worktree.thread_id).await?.map_or_else(
                || worktree.thread_id.clone(),
                |thread| display_name(&thread),
            ),
        };
        let dir = Path::new(&worktree.dir);
        let standing = match git::apart(dir, &worktree.base, &worktree.branch).await {
            Ok((ahead, behind)) => {
                let unsaved = git::unsaved(dir, true).await.map_or(0, |files| files.len());
                format!("{ahead} ahead, {behind} behind, {unsaved} uncommitted")
            }
            Err(error) => format!("not readable: {error:#}"),
        };
        lines.push(format!(
            "- {} ({who}) from {}, in {}: {standing}",
            worktree.branch, worktree.base, worktree.dir
        ));
    }
    Ok(printable(&lines.join("\n")))
}

/// Removes the clean worktree, the branch when its work is merged, and the record: the
/// thread works where it started again. What was kept is said.
async fn finish(state: &State, worktree: &ThreadWorktree) -> Result<String> {
    let dir = Path::new(&worktree.dir);
    let origin = Path::new(&worktree.worktree_of);
    git::remove_worktree(&folder(origin), &top(dir).await?).await?;
    state.remove_thread_worktree(&worktree.thread_id).await?;
    let removed = git::remove_merged(origin, &worktree.branch).await?;
    let branch = match removed {
        true => format!("the merged branch {} is deleted", worktree.branch),
        false => format!("the branch {} is kept with its work", worktree.branch),
    };
    Ok(format!(
        "The worktree is removed and {branch}; work goes on in {}.",
        origin.display()
    ))
}

/// The top of the checkout `dir` is in.
async fn top(dir: &Path) -> Result<PathBuf> {
    Ok(git::repository(dir)
        .await
        .context("the worktree is no repository")?
        .top)
}

/// Whether a file still has the lines git marks a conflict with. A file that cannot be
/// read, such as one deleted on a side, has none.
fn has_markers(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| {
        String::from_utf8_lossy(&bytes)
            .lines()
            .any(|line| line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> "))
    })
}

async fn conflicts(dir: &Path) -> Result<Vec<String>> {
    Ok(git::info(&folder(&top(dir).await?))
        .await?
        .changes
        .into_iter()
        .filter(|change| change.kind == ChangeKind::Conflicted)
        .map(|change| change.path)
        .collect())
}

fn listed(files: &[String]) -> String {
    let mut shown: Vec<&str> = files.iter().take(MAX_LISTED).map(String::as_str).collect();
    let more = files.len().saturating_sub(MAX_LISTED);
    let more = format!("and {more} more");
    if files.len() > MAX_LISTED {
        shown.push(&more);
    }
    match shown.is_empty() {
        true => "none".to_owned(),
        false => shown.join(", "),
    }
}

fn plural(count: u64) -> &'static str {
    match count {
        1 => "",
        _ => "s",
    }
}

fn text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .context("the folder is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::free_name;

    #[test]
    fn a_branch_name_is_plain_and_free() {
        let taken = vec!["agent/fix-login".to_owned()];
        assert_eq!(
            free_name(Some("Fix login!"), "t", &taken).unwrap(),
            "agent/fix-login-2"
        );
        assert_eq!(free_name(Some("../../-x"), "t", &[]).unwrap(), "agent/x");
        assert_eq!(
            free_name(None, "0195f00d-1234-7abc-8def-0123456789ab", &[]).unwrap(),
            "agent/456789ab"
        );
        assert!(free_name(Some("--"), "t", &[]).is_err());
    }
}
