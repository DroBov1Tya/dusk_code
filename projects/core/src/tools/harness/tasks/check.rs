//! The lead's tasks made into what the plan's store takes, with the checks the store
//! cannot make: files on disk, roles in the agents catalog, the gate as a goal check, and
//! a loop among the keys of one `add` named by those keys. The store checks shapes, caps
//! and loops through tasks already in the plan, before it writes anything.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::text::plain;
use crate::{
    catalog::agents::{Agents, GENERAL_AGENT},
    config::Effort,
    project::plain_relative,
    runtime::tiers::Tier as ModelTier,
    state::{GoalCheck, MAX_FILES, NewTask, TaskEdit, TaskFiles},
    tools::{ToolContext, ToolScope},
};

/// Known roles named in a refusal at most.
const MAX_NAMED_ROLES: usize = 24;

/// A task as the lead writes it, in `add` or in `edit`. A field left out of an `edit`
/// stays as it is.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskArguments {
    pub text: String,
    pub key: Option<String>,
    pub after: Option<Vec<String>>,
    pub role: Option<String>,
    pub files: Option<Vec<String>>,
    pub accept: Option<String>,
    pub gate: Option<String>,
    pub confirm: Option<bool>,
    pub brief: Option<String>,
    pub phase: Option<String>,
    pub kind: Option<String>,
    pub tier: Option<ModelTier>,
    pub effort: Option<Effort>,
}

impl TaskArguments {
    /// How errors name it: its key, else its place in the call.
    fn label(&self, index: usize) -> String {
        match &self.key {
            Some(key) => format!("task {}", plain(key, 64)),
            None => format!("task {}", index + 1),
        }
    }
}

/// The tasks of an `add`, checked against the disk and the catalog.
pub async fn new_tasks(
    entries: Vec<TaskArguments>,
    context: &ToolContext,
    agents: &Agents,
) -> Result<Vec<NewTask>> {
    refuse_key_loop(&entries)?;
    let mut tasks = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let label = entry.label(index);
        let parts = parts(&entry, context.scope.as_ref()).with_context(|| label.clone())?;
        tasks.push(NewTask {
            key: entry.key,
            description: entry.text,
            after: entry.after.unwrap_or_default(),
            role: parts.role,
            phase: entry.phase.unwrap_or_default(),
            acceptance: entry.accept.unwrap_or_default(),
            brief: entry.brief.unwrap_or_default(),
            gate: parts.gate.flatten(),
            files: parts.files.unwrap_or_default(),
            tier: entry.tier,
            effort: entry.effort,
        });
    }
    let roles: Vec<&str> = tasks
        .iter()
        .filter_map(|task| task.role.as_deref())
        .collect();
    known_roles(&roles, context, agents).await?;
    Ok(tasks)
}

/// What an `edit` changes, checked as `add` checks it.
pub async fn task_edit(
    entry: TaskArguments,
    context: &ToolContext,
    agents: &Agents,
) -> Result<TaskEdit> {
    ensure!(
        entry.key.is_none(),
        "a key names a task only inside one add; give `after` as ids"
    );
    let parts = parts(&entry, context.scope.as_ref())?;
    if let Some(Some(role)) = &parts.role_given {
        known_roles(&[role.as_str()], context, agents).await?;
    }
    Ok(TaskEdit {
        description: Some(entry.text),
        after: entry.after,
        role: parts.role_given,
        phase: entry.phase,
        acceptance: entry.accept,
        brief: entry.brief,
        gate: parts.gate,
        files: parts.files,
        tier: entry.tier,
        effort: entry.effort,
    })
}

/// The fields of a task that need more than copying.
struct Parts {
    /// The role as stored: `None` for the general agent.
    role: Option<String>,
    /// For an edit: `None` when the role was left out.
    role_given: Option<Option<String>>,
    /// `None` when neither gate nor confirm was given.
    gate: Option<Option<GoalCheck>>,
    files: Option<TaskFiles>,
}

fn parts(entry: &TaskArguments, scope: &ToolScope) -> Result<Parts> {
    if entry.kind.as_deref().is_some_and(|kind| !kind.is_empty()) {
        bail!("`kind` needs procedures, which are not built yet; leave it out");
    }
    let role_given = entry
        .role
        .as_deref()
        .map(|role| (role != GENERAL_AGENT).then(|| role.to_owned()));
    let files = entry
        .files
        .as_deref()
        .map(|files| project_files(scope, files))
        .transpose()?;
    Ok(Parts {
        role: role_given.clone().flatten(),
        role_given,
        gate: gate(entry.gate.as_deref(), entry.confirm)?,
        files,
    })
}

/// `gate` is a command the core runs, `confirm` the user's yes; the goal's rule decides
/// what a command may be.
fn gate(command: Option<&str>, confirm: Option<bool>) -> Result<Option<Option<GoalCheck>>> {
    let check = match (command, confirm) {
        (Some(_), Some(true)) => bail!("give `gate` or `confirm`, not both"),
        (Some(command), _) => GoalCheck::Command {
            command: command.to_owned(),
        },
        (None, Some(true)) => GoalCheck::UserConfirmation,
        (None, Some(false)) => return Ok(Some(None)),
        (None, None) => return Ok(None),
    };
    check
        .validate()
        .context("the gate must be the exact command that exits 0 when the task is done")?;
    Ok(Some(Some(check)))
}

/// Each entry relative to the project, resolved with links followed, and refused when
/// that leads out of it. A folder that exists gets its `/`, so it overlaps what is in it.
fn project_files(scope: &ToolScope, entries: &[String]) -> Result<TaskFiles> {
    ensure!(
        entries.len() <= MAX_FILES,
        "more than {MAX_FILES} files in a task; name a folder that holds them"
    );
    let mut resolved = Vec::with_capacity(entries.len());
    for entry in entries {
        // The shape first, so nothing on disk is asked about a pattern or `..`.
        TaskFiles::new([entry.as_str()])?;
        resolved.push(project_file(scope, entry)?);
    }
    Ok(TaskFiles::new(resolved)?)
}

fn project_file(scope: &ToolScope, entry: &str) -> Result<String> {
    let outside = || {
        format!(
            "{}: the path must stay inside the project",
            plain(entry, 80)
        )
    };
    let path = entry.trim_end_matches('/');
    plain_relative(Path::new(path)).with_context(outside)?;
    let (resolved, _) = scope
        .locate(path)
        .with_context(|| format!("{}: cannot resolve the path", plain(entry, 80)))?;
    let inside = resolved
        .strip_prefix(scope.cwd())
        .ok()
        .with_context(outside)?;
    let parts: Vec<&str> = inside
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<_>>()
        .context("the path is not valid UTF-8")?;
    ensure!(
        !parts.is_empty(),
        "{}: the project as a whole cannot be an entry; name its files or folders",
        plain(entry, 80)
    );
    let mut relative = parts.join("/");
    if entry.ends_with('/') || resolved.is_dir() {
        relative.push('/');
    }
    Ok(relative)
}

/// Every role is an agent of the catalog that is switched on. A role not known yet may
/// have just been written, so the catalog is read again once before the refusal.
async fn known_roles(roles: &[&str], context: &ToolContext, agents: &Agents) -> Result<()> {
    let unknown = |roles: &[&str]| -> Vec<String> {
        roles
            .iter()
            .filter(|role| agents.get(role).is_none() || !context.config.agent_enabled(role))
            .map(|role| (*role).to_owned())
            .collect()
    };
    if unknown(roles).is_empty() {
        return Ok(());
    }
    agents.scan().await.context("cannot read the agents")?;
    let Some(role) = unknown(roles).into_iter().next() else {
        return Ok(());
    };
    let mut known: Vec<String> = agents
        .names()
        .into_iter()
        .filter(|name| context.config.agent_enabled(name) && name != GENERAL_AGENT)
        .take(MAX_NAMED_ROLES)
        .collect();
    known.insert(0, GENERAL_AGENT.to_owned());
    bail!(
        "no agent {}; roles are: {}. Leave role out for general, or have an agent write \
         agents/<name>/AGENT.md first",
        plain(&role, 64),
        known.join(", ")
    )
}

/// A loop among the keys of one `add`. Tasks already in the plan cannot depend on new
/// ones, so this is every loop an `add` can make; named by keys, which the lead knows.
fn refuse_key_loop(entries: &[TaskArguments]) -> Result<()> {
    let keys: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| Some((entry.key.as_deref()?, index)))
        .collect();
    let edges: Vec<Vec<usize>> = entries
        .iter()
        .map(|entry| {
            entry
                .after
                .iter()
                .flatten()
                .filter_map(|after| keys.get(after.as_str()).copied())
                .collect()
        })
        .collect();
    // 0 not seen, 1 on the path, 2 done.
    let mut seen = vec![0u8; entries.len()];
    for start in 0..entries.len() {
        let mut path = Vec::new();
        if let Some(cycle) = walk(start, &edges, &mut seen, &mut path) {
            let name = |index: usize| plain(entries[index].key.as_deref().unwrap_or_default(), 64);
            bail!(
                "{}; take one of them out of `after`",
                describe(&cycle, name)
            );
        }
    }
    Ok(())
}

/// Depth first; answers the tasks of the first loop found, from a task back to itself.
fn walk(
    node: usize,
    edges: &[Vec<usize>],
    seen: &mut [u8],
    path: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    match seen[node] {
        2 => return None,
        1 => {
            let from = path.iter().position(|&on| on == node)?;
            let mut cycle = path[from..].to_vec();
            cycle.push(node);
            return Some(cycle);
        }
        _ => {}
    }
    seen[node] = 1;
    path.push(node);
    for &next in &edges[node] {
        if let Some(cycle) = walk(next, edges, seen, path) {
            return Some(cycle);
        }
    }
    path.pop();
    seen[node] = 2;
    None
}

/// `task #a depends on #b, which depends on #a`, as the store says it with ids.
fn describe(cycle: &[usize], name: impl Fn(usize) -> String) -> String {
    if cycle.len() <= 2 {
        return format!("task #{} depends on itself", name(cycle[0]));
    }
    let mut text = format!("task #{}", name(cycle[0]));
    for (index, next) in cycle[1..].iter().enumerate() {
        let link = if index == 0 {
            " depends on"
        } else {
            ", which depends on"
        };
        text.push_str(&format!("{link} #{}", name(*next)));
    }
    text
}
