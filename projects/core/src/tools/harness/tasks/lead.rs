//! The lead's actions on the plan: add the whole plan, edit, show, review, fail, reopen,
//! and run or stop the team. Each answers one line, except `show`. Everything the lead
//! writes is model text: checked here and in the store, and made plain where echoed.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;

use super::{
    check::{self, TaskArguments},
    team::{GatesYes, TeamRunner},
    text::{self, Shown},
};
use crate::{
    catalog::agents::Agents,
    state::{
        MAX_GOAL, MAX_OPEN_TASKS, MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PassedBy, PlanError,
        PlanStatus, PlanTask, State, TaskEdit, short_task_id, timestamp_ms,
    },
    tools::ToolContext,
};

/// The answer to `stop`: the team winds down, it does not end at once.
pub(super) const STOPPING: &str = "stopping: the members at work hand over first; you are \
                                   told when the team ended";

/// What a chat's `tasks` needs for the plan form, handed in by the launcher.
pub struct Lead {
    pub(super) agents: Arc<Agents>,
    pub(super) team: Arc<dyn TeamRunner>,
}

/// The plan form's arguments; the schema the lead reads is `text::schema`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanArguments {
    action: PlanAction,
    tasks: Option<Vec<TaskArguments>>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    text: Option<String>,
    goal: Option<String>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PlanAction {
    Add,
    Run,
    Review,
    Show,
    Edit,
    Fail,
    Reopen,
    Skip,
    Stop,
}

/// What the lead is answered, and whether the plan changed, so clients are told.
pub struct Answer {
    pub text: String,
    pub changed: bool,
}

fn changed(text: String) -> Answer {
    Answer {
        text,
        changed: true,
    }
}

impl PlanAction {
    fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Run => "run",
            Self::Review => "review",
            Self::Show => "show",
            Self::Edit => "edit",
            Self::Fail => "fail",
            Self::Reopen => "reopen",
            Self::Skip => "skip",
            Self::Stop => "stop",
        }
    }

    /// The fields the action reads besides `action`.
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Add => &["tasks"],
            Self::Run => &["goal"],
            Self::Stop => &[],
            Self::Review => &["id", "ids", "text"],
            Self::Show => &["id"],
            Self::Reopen => &["id", "ids"],
            Self::Edit => &["id", "ids", "tasks", "text"],
            Self::Fail | Self::Skip => &["id", "text"],
        }
    }
}

impl PlanArguments {
    /// A field the action does not read is refused, not ignored: the lead meant something.
    fn refuse_unused(&self) -> Result<()> {
        let given = [
            ("tasks", self.tasks.is_some()),
            ("id", self.id.is_some()),
            ("ids", self.ids.is_some()),
            ("text", self.text.is_some()),
            ("goal", self.goal.is_some()),
        ];
        for (field, present) in given {
            ensure!(
                !present || self.action.fields().contains(&field),
                "{} does not take `{field}`",
                self.action.name()
            );
        }
        Ok(())
    }
}

/// A field the action needs.
fn needed<T>(field: Option<T>, name: &str, action: &str) -> Result<T> {
    field.with_context(|| format!("{action} needs `{name}`"))
}

impl Lead {
    pub fn new(agents: Arc<Agents>, team: Arc<dyn TeamRunner>) -> Self {
        Self { agents, team }
    }

    pub async fn call(
        &self,
        state: &State,
        thread: &str,
        arguments: PlanArguments,
        context: &ToolContext,
    ) -> Result<Answer> {
        arguments.refuse_unused()?;
        let PlanArguments {
            action,
            tasks,
            id,
            ids,
            text,
            goal,
        } = arguments;
        let name = action.name();
        match action {
            PlanAction::Add => {
                let tasks = needed(tasks, "tasks", name)?;
                self.add(state, thread, tasks, context).await
            }
            PlanAction::Edit => {
                let ids = one_or_many(id, ids, name)?;
                self.edit(state, thread, &ids, tasks, text, context).await
            }
            PlanAction::Show => match id {
                Some(id) => show_task(state, thread, &id).await,
                None => self.show(state, thread).await,
            },
            PlanAction::Review => review(state, thread, id, ids, text).await,
            PlanAction::Fail => fail(state, thread, &needed(id, "id", name)?, text).await,
            PlanAction::Reopen => reopen(state, thread, &one_or_many(id, ids, name)?).await,
            PlanAction::Skip => skip(state, thread, &needed(id, "id", name)?, text).await,
            PlanAction::Run => {
                let goal = needed(goal, "goal", name)?;
                self.run(state, thread, &goal, context).await
            }
            PlanAction::Stop => {
                self.team.stop(thread).await?;
                Ok(changed(STOPPING.into()))
            }
        }
    }

    /// The whole plan in one call. A call run again, as after a restart, finds its tasks
    /// already in the plan and answers as the first time.
    async fn add(
        &self,
        state: &State,
        thread: &str,
        tasks: Vec<TaskArguments>,
        context: &ToolContext,
    ) -> Result<Answer> {
        ensure!(
            !tasks.is_empty(),
            "add needs `tasks`: the whole plan, [{{\"text\": ...}}, ...]"
        );
        ensure!(
            tasks.len() <= MAX_TASKS_AN_ADD,
            "more than {MAX_TASKS_AN_ADD} tasks in one add; add the first phase now"
        );
        let new = check::new_tasks(tasks, context, &self.agents).await?;
        let plan = state.plan(thread).await.map_err(explain)?;
        if let Some(ids) = replayed(&new, &plan)? {
            return Ok(changed(added(&ids)));
        }
        let added_tasks = state.add_plan_tasks(thread, &new).await.map_err(explain)?;
        let ids: Vec<&str> = added_tasks
            .iter()
            .map(|task| task.task_id.as_str())
            .collect();
        Ok(changed(added(&ids)))
    }

    /// Changes every field given of one task, or one field set of several at once (`ids`,
    /// such as the gate of every task in the queue). Open, failed and blocked tasks take
    /// it; a failed one stays failed until `reopen`.
    async fn edit(
        &self,
        state: &State,
        thread: &str,
        ids: &[String],
        tasks: Option<Vec<TaskArguments>>,
        text: Option<String>,
        context: &ToolContext,
    ) -> Result<Answer> {
        let many = ids.len() > 1;
        let edit = match (tasks, text) {
            (Some(_), Some(_)) => bail!("edit takes `text`, or one task in `tasks`, not both"),
            (Some(mut tasks), None) => {
                ensure!(
                    tasks.len() == 1,
                    "edit takes one task in `tasks`; with `ids` its fields go to each"
                );
                let task = tasks.remove(0);
                if many {
                    ensure!(
                        task.text.is_none() && task.files.is_none(),
                        "an edit of several `ids` changes the same fields of each; `text` and \
                         `files` are each task's own, so edit them one by one"
                    );
                }
                check::task_edit(task, context, &self.agents).await?
            }
            (None, Some(_)) if many => {
                bail!("`text` is each task's own; edit several `ids` with one task in `tasks`")
            }
            (None, Some(text)) => TaskEdit {
                description: Some(text),
                ..TaskEdit::default()
            },
            (None, None) => bail!("edit needs `text`, or one task in `tasks` with what changes"),
        };
        ensure!(
            edit != TaskEdit::default(),
            "edit needs what changes: a field of the task in `tasks`"
        );
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let edited = match state.edit_plan_tasks(thread, &ids, &edit).await {
            Ok(edited) => edited,
            Err(PlanError::Illegal { task, from, .. }) => bail!(not_editable(&task, from)),
            Err(error) => return Err(explain(error)),
        };
        let failed = edited
            .iter()
            .filter(|task| task.status == PlanStatus::Failed)
            .count();
        let ids: Vec<String> = edited
            .iter()
            .map(|task| format!("#{}", short_task_id(&task.task_id)))
            .collect();
        let mut line = format!("edited {}", ids.join(" "));
        if failed > 0 {
            line.push_str("; reopen a failed one to retry it");
        }
        Ok(changed(line))
    }

    async fn show(&self, state: &State, thread: &str) -> Result<Answer> {
        let plan = state.plan(thread).await.map_err(explain)?;
        let mut shown = Vec::with_capacity(plan.len());
        for task in &plan {
            shown.push(Shown {
                task,
                member: member(state, task).await?,
            });
        }
        let team = self.team.team(thread).await?;
        Ok(Answer {
            text: text::plan(&shown, team.as_ref()),
            changed: false,
        })
    }

    async fn run(
        &self,
        state: &State,
        thread: &str,
        goal: &str,
        context: &ToolContext,
    ) -> Result<Answer> {
        let goal = goal.trim();
        ensure!(
            !goal.is_empty(),
            "run needs `goal`: one sentence, and how you will know it is done"
        );
        ensure!(
            goal.chars().count() <= MAX_GOAL,
            "the goal is longer than {MAX_GOAL} characters; write it shorter"
        );
        let plan = state.plan(thread).await.map_err(explain)?;
        ensure!(
            plan.iter().any(|task| !task.skipped
                && !matches!(task.status, PlanStatus::Done | PlanStatus::Failed)),
            "the plan has nothing left to do; add tasks first, or reopen a failed one"
        );
        let started = self
            .team
            .run(thread, goal, context.progress.clone())
            .await?;
        let gates = match started.gates {
            GatesYes::Refused => {
                "; the user did not allow its gate commands, so a gated task waits for your \
                 review"
            }
            GatesYes::Allowed | GatesYes::NotAsked => "",
        };
        Ok(changed(format!(
            "running: {} given out{gates}",
            started.given
        )))
    }
}

/// `added 7 tasks: #5d01aa3c #77aa01bc ...`, in the order of the add.
fn added(ids: &[&str]) -> String {
    let noun = if ids.len() == 1 { "task" } else { "tasks" };
    let ids: Vec<String> = ids
        .iter()
        .map(|id| format!("#{}", short_task_id(id)))
        .collect();
    format!("added {} {noun}: {}", ids.len(), ids.join(" "))
}

/// The tasks of the plan this add already made, when it is a call run again: every task
/// of it has a twin not done, each twin used once. Some twins and not all is a task
/// written twice, which is refused.
fn replayed<'a>(new: &[NewTask], plan: &'a [PlanTask]) -> Result<Option<Vec<&'a str>>> {
    let mut twins: Vec<&str> = Vec::new();
    let mut first_twin = None;
    let mut missing = false;
    for (index, task) in new.iter().enumerate() {
        let twin = plan.iter().find(|old| {
            old.status != PlanStatus::Done
                && !twins.contains(&old.task_id.as_str())
                && same(task, old)
        });
        match twin {
            Some(old) => {
                twins.push(&old.task_id);
                first_twin.get_or_insert((index, old.task_id.as_str()));
            }
            None => missing = true,
        }
    }
    match (missing, first_twin) {
        (false, _) => Ok(Some(twins)),
        (true, None) => Ok(None),
        (true, Some((index, id))) => bail!(
            "task {} is the same as #{} already in the plan; leave it out, or reopen #{} \
             if it failed",
            index + 1,
            short_task_id(id),
            short_task_id(id)
        ),
    }
}

fn same(new: &NewTask, old: &PlanTask) -> bool {
    new.description.trim() == old.description
        && new.role == old.role
        && new.phase == old.phase
        && new.acceptance == old.acceptance
        && new.brief == old.brief
        && new.gate == old.gate
        && new.files == old.files
        && new.tier == old.tier
        && new.effort == old.effort
}

/// The member that holds a task, by the name its thread goes by.
async fn member(state: &State, task: &PlanTask) -> Result<Option<String>> {
    let holds = matches!(
        task.status,
        PlanStatus::Claimed | PlanStatus::Running | PlanStatus::Blocked | PlanStatus::Review
    );
    let Some(owner) = task.owner_thread_id.as_deref().filter(|_| holds) else {
        return Ok(None);
    };
    Ok(state
        .load_thread(owner)
        .await?
        .and_then(|thread| thread.name))
}

async fn show_task(state: &State, thread: &str, id: &str) -> Result<Answer> {
    let task = state.plan_task(thread, id).await.map_err(explain)?;
    let plan = state.plan(thread).await.map_err(explain)?;
    let after: Vec<(String, Option<PlanStatus>)> = task
        .depends_on
        .iter()
        .map(|id| {
            let status = plan.iter().find(|task| &task.task_id == id);
            (id.clone(), status.map(|task| task.status))
        })
        .collect();
    let shown = Shown {
        member: member(state, &task).await?,
        task: &task,
    };
    Ok(Answer {
        text: text::task(&shown, &after),
        changed: false,
    })
}

/// Accepts tasks in review, or sends one back with what to redo. Run again, an accepted
/// task is done and a rejected one is at work with the same note: both answer as before.
async fn review(
    state: &State,
    thread: &str,
    id: Option<String>,
    ids: Option<Vec<String>>,
    text: Option<String>,
) -> Result<Answer> {
    match (id, ids, text) {
        (None, Some(ids), None) => accept(state, thread, &ids).await,
        (Some(id), None, Some(text)) => reject(state, thread, &id, &text).await,
        _ => bail!("review accepts `ids`, or rejects one `id` with `text` to redo"),
    }
}

async fn accept(state: &State, thread: &str, ids: &[String]) -> Result<Answer> {
    ensure!(!ids.is_empty(), "review needs `ids` to accept");
    ensure!(
        ids.len() <= MAX_OPEN_TASKS,
        "more than {MAX_OPEN_TASKS} ids in one review"
    );
    let mut accepted: Vec<String> = Vec::new();
    for id in ids {
        let task_id = match state.pass_task(thread, id, PassedBy::Lead).await {
            Ok(task) => task.task_id,
            Err(PlanError::Illegal {
                task,
                from: PlanStatus::Done,
                ..
            }) => task,
            Err(PlanError::Illegal { task, from, .. }) => {
                return Err(partly(
                    &accepted,
                    anyhow!(
                        "task #{} is {}; review takes a task in review, which show marks [?]",
                        short_task_id(&task),
                        from.as_str()
                    ),
                ));
            }
            Err(error) => return Err(partly(&accepted, explain(error))),
        };
        accepted.push(format!("#{}", short_task_id(&task_id)));
    }
    Ok(changed(format!("accepted {}", accepted.join(" "))))
}

/// An error after some tasks were accepted says which, so the lead does not repeat them.
fn partly(accepted: &[String], error: anyhow::Error) -> anyhow::Error {
    if accepted.is_empty() {
        return error;
    }
    anyhow!("accepted {}, then: {error:#}", accepted.join(" "))
}

async fn reject(state: &State, thread: &str, id: &str, text: &str) -> Result<Answer> {
    ensure!(!text.trim().is_empty(), "review needs `text`: what to redo");
    let task = match state.rework_task(thread, id, text, timestamp_ms()?).await {
        Ok(task) => task,
        Err(PlanError::Illegal { task, from, .. }) => {
            let found = state.plan_task(thread, &task).await.map_err(explain)?;
            let again =
                matches!(from, PlanStatus::Running | PlanStatus::Failed) && found.note == text;
            ensure!(
                again,
                "task #{} is {}; review takes a task in review, which show marks [?]",
                short_task_id(&task),
                from.as_str()
            );
            found
        }
        Err(error) => return Err(explain(error)),
    };
    let line = match task.status {
        PlanStatus::Failed => format!(
            "#{} failed: sent back {MAX_REWORKS} times already",
            short_task_id(&task.task_id)
        ),
        _ => format!(
            "sent back #{} to redo ({} of {MAX_REWORKS})",
            short_task_id(&task.task_id),
            task.reworks
        ),
    };
    Ok(changed(line))
}

/// Run again on a failed task, it is already so.
async fn fail(state: &State, thread: &str, id: &str, note: Option<String>) -> Result<Answer> {
    let task_id = match state
        .fail_task(thread, id, note.as_deref().unwrap_or_default())
        .await
    {
        Ok(task) => task.task_id,
        Err(PlanError::Illegal {
            task,
            from: PlanStatus::Failed,
            ..
        }) => task,
        Err(PlanError::Illegal {
            task,
            from: PlanStatus::Done,
            ..
        }) => bail!(
            "task #{} is done, which is final; add a new task for more work",
            short_task_id(&task)
        ),
        Err(error) => return Err(explain(error)),
    };
    Ok(changed(format!("failed #{}", short_task_id(&task_id))))
}

/// Gives up a failed or blocked task, so the wave after it can start. Run again on a
/// skipped task, it is already so.
async fn skip(state: &State, thread: &str, id: &str, note: Option<String>) -> Result<Answer> {
    let note = note.unwrap_or_default();
    let skipped = match state.skip_task(thread, id, &note).await {
        Ok(skipped) => skipped,
        Err(PlanError::Illegal { task, from, .. }) => {
            let found = state.plan_task(thread, &task).await.map_err(explain)?;
            ensure!(
                found.skipped,
                "task #{} is {}; skip takes a failed or blocked task",
                short_task_id(&task),
                from.as_str()
            );
            vec![found]
        }
        Err(error) => return Err(explain(error)),
    };
    let ids: Vec<String> = skipped
        .iter()
        .map(|task| format!("#{}", short_task_id(&task.task_id)))
        .collect();
    Ok(changed(match ids.as_slice() {
        [one] => format!("skipped {one}"),
        [first, behind @ ..] => format!("skipped {first}, and {} behind it", behind.join(" ")),
        [] => "skipped nothing".into(),
    }))
}

/// Retries failed tasks in place, or frees blocked ones. Run again on an open task, it
/// is already so. An error after some were reopened says which.
async fn reopen(state: &State, thread: &str, ids: &[String]) -> Result<Answer> {
    let mut reopened: Vec<String> = Vec::new();
    for id in ids {
        let task_id = match state.reopen_task(thread, id).await {
            Ok(task) => task.task_id,
            Err(PlanError::Illegal {
                task,
                from: PlanStatus::Open,
                ..
            }) => task,
            Err(PlanError::Illegal { task, from, .. }) => {
                let refused = anyhow!(
                    "task #{} is {}; reopen takes a failed or blocked task",
                    short_task_id(&task),
                    from.as_str()
                );
                return Err(partly_reopened(&reopened, refused));
            }
            Err(error) => return Err(partly_reopened(&reopened, explain(error))),
        };
        let task = state.plan_task(thread, &task_id).await.map_err(explain)?;
        let mut word = format!("#{}", short_task_id(&task_id));
        if task.retries > 0 {
            word.push_str(&format!(" (try {})", task.retries + 1));
        }
        reopened.push(word);
    }
    Ok(changed(format!("reopened {}", reopened.join(" "))))
}

fn partly_reopened(reopened: &[String], error: anyhow::Error) -> anyhow::Error {
    if reopened.is_empty() {
        return error;
    }
    anyhow!("reopened {}, then: {error:#}", reopened.join(" "))
}

/// Why `edit` cannot change a task in `from`, and the call that gets past it.
fn not_editable(task: &str, from: PlanStatus) -> String {
    let id = short_task_id(task);
    match from {
        PlanStatus::Done => {
            format!("task #{id} is done, which is final; add a new task for more work")
        }
        PlanStatus::Blocked => {
            format!("task #{id} is blocked and its member waits; reopen it first, then edit it")
        }
        PlanStatus::Review => {
            format!("task #{id} is in review; review or fail it first, then edit it and reopen it")
        }
        _ => format!(
            "task #{id} is {}: its member works from the card as it is; fail it first, \
             then edit it and reopen it",
            from.as_str()
        ),
    }
}

/// `id`, or `ids` for several at once; not both, and at most a plan's worth.
fn one_or_many(id: Option<String>, ids: Option<Vec<String>>, action: &str) -> Result<Vec<String>> {
    match (id, ids) {
        (Some(_), Some(_)) => bail!("{action} takes `id` or `ids`, not both"),
        (Some(id), None) => Ok(vec![id]),
        (None, Some(ids)) => {
            ensure!(!ids.is_empty(), "{action} needs `id`, or `ids` not empty");
            ensure!(
                ids.len() <= MAX_OPEN_TASKS,
                "more than {MAX_OPEN_TASKS} ids in one {action}"
            );
            Ok(ids)
        }
        (None, None) => bail!("{action} needs `id`, or `ids` for several"),
    }
}

/// The store's refusal with the call that gets past it.
fn explain(error: PlanError) -> anyhow::Error {
    let hint = match &error {
        PlanError::NoSuchTask(_) | PlanError::Ambiguous(_) => "; show prints the ids",
        PlanError::Cycle(_) => "; take one of them out of `after`",
        PlanError::UnknownDependency(_) => {
            "; `after` takes keys of this add, or ids that show prints"
        }
        PlanError::DuplicateKey(_) => "; give each task its own key",
        PlanError::TooLong { .. } => "; write it shorter",
        _ => "",
    };
    anyhow!("{error}{hint}")
}
