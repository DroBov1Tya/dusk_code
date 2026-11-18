//! `tasks`: the agent's own board for the work of a thread. It writes down the tasks it
//! set itself with their steps, looks at the board, rewrites a task or a step, and marks
//! a step or a whole task done. Steps are timed as they run. The board lives in the
//! database, so it outlives a compaction.
//!
//! A chat in TEAM mode (unless `limits.plan_graph` is off) is offered the plan form
//! instead (`lead`): the same rows as a plan the core runs with a team. The board's
//! `add_items`, `start` and `done` are still taken there, never on a task the team has
//! given out. Anywhere else the tool is byte for byte the board.

mod check;
mod lead;
mod team;
mod text;

use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

pub use team::{GatesYes, Started, TeamRunner};
pub use text::short as short_task_id;

use lead::Lead;
use team::NoTeam;

use crate::tools::{Tool, ToolContext, schema, schema::parse_arguments};
use crate::{
    BoxFuture,
    catalog::{agents::Agents, modes::TEAM},
    runtime::ThreadEvent,
    state::{PlanError, PlanStatus, State, Task},
};

/// IDs are shown by their last characters, which are random; the agent may use those.
const SHORT_ID: usize = 8;

pub struct Tasks {
    state: Arc<State>,
    thread_id: String,
    /// A chat's, set by the launcher; without it only the board is offered.
    lead: Option<Lead>,
}

/// Actions only the plan form has, and only the board has.
const PLAN_ONLY: [&str; 5] = ["run", "review", "fail", "reopen", "stop"];
const BOARD_ONLY: [&str; 3] = ["add_items", "start", "done"];

fn action(arguments: &Value) -> &str {
    arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Flat, since providers want one object at the top of a schema; `run` checks which
/// fields each action needs.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TasksArguments {
    action: Action,
    /// For add: the task in a few words.
    task: Option<String>,
    /// For add and add_items: the steps, in order.
    #[serde(default)]
    items: Vec<Step>,
    /// For add_items: a task. For start: a step. For done and edit: a task or a step.
    id: Option<String>,
    /// For edit: the new words.
    text: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    // A new task with its steps.
    Add,
    // More steps for a task.
    AddItems,
    Show,
    // A step begins out of order; otherwise each starts when the one before is done.
    Start,
    // A step, or a whole task with all its steps.
    Done,
    // New words for a task or a step.
    Edit,
}

// Models often write a step as an object with its text; both mean the same.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum Step {
    Text(String),
    Object { text: String },
}

impl Step {
    fn into_text(self) -> String {
        match self {
            Self::Text(text) | Self::Object { text } => text,
        }
    }
}

/// A field the action needs.
fn needed(field: Option<String>, name: &str, action: &str) -> anyhow::Result<String> {
    field.with_context(|| format!("{action} needs {name}"))
}

impl Tasks {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self {
            state,
            thread_id,
            lead: None,
        }
    }

    /// The tasks of a chat, which lead a team when the plan form is on; `agents` are the
    /// roles a task may name. Until `with_team` the plan cannot run.
    // The launcher's to call, for a chat only.
    pub fn lead(self, agents: Arc<Agents>) -> Self {
        Self {
            lead: Some(Lead::new(agents, Arc::new(NoTeam))),
            ..self
        }
    }

    /// What runs the plan of a chat's `tasks`.
    pub fn with_team(mut self, team: Arc<dyn TeamRunner>) -> Self {
        if let Some(lead) = &mut self.lead {
            lead.team = team;
        }
        self
    }

    /// The plan form, when this is a chat's and its mode is TEAM. `limits.plan_graph`
    /// is the kill switch.
    fn plan_form(&self, context: &ToolContext) -> Option<&Lead> {
        let lead = self.lead.as_ref()?;
        let config = &context.config;
        (config.limits().plan_graph && config.mode() == TEAM).then_some(lead)
    }

    /// The board's actions do not touch a task the team has given out: its review, the
    /// lead's fail or reopen, or the core moves it.
    async fn refuse_given_out(&self, id: &str) -> anyhow::Result<()> {
        let task = match self.state.plan_task(&self.thread_id, id).await {
            Ok(task) => task,
            // A step, or no task: the board answers for those.
            Err(
                PlanError::NoSuchTask(_) | PlanError::Ambiguous(_) | PlanError::NoSuchThread(_),
            ) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        // A plan is found from the chat; an agent's own board is its thread's alone.
        if task.plan != self.thread_id {
            return Ok(());
        }
        let given_out = !matches!(task.status, PlanStatus::Open | PlanStatus::Done)
            || task.owner_thread_id.is_some();
        ensure!(
            !given_out,
            "task #{} is {} in the team's plan; the board cannot change it",
            text::short(&task.task_id),
            task.status.as_str()
        );
        Ok(())
    }

    async fn run(&self, arguments: TasksArguments) -> anyhow::Result<Vec<Task>> {
        let thread = &self.thread_id;
        let TasksArguments {
            action,
            task,
            items,
            id,
            text,
        } = arguments;
        let items: Vec<String> = items.into_iter().map(Step::into_text).collect();
        if let (Some(id), Action::AddItems | Action::Start | Action::Done | Action::Edit) =
            (&id, &action)
        {
            self.refuse_given_out(id).await?;
        }
        match action {
            Action::Add => {
                let task = needed(task, "task", "add")?;
                self.state.add_task(thread, &task, &items).await?;
            }
            Action::AddItems => {
                let id = needed(id, "id", "add_items")?;
                ensure!(!items.is_empty(), "add_items needs items");
                self.state.add_task_items(thread, &id, &items).await?;
            }
            Action::Start => {
                let id = needed(id, "id", "start")?;
                self.state.start_item(thread, &id).await?;
            }
            Action::Done => {
                let id = needed(id, "id", "done")?;
                self.state.complete(thread, &id).await?;
            }
            Action::Edit => {
                let id = needed(id, "id", "edit")?;
                let text = needed(text, "text", "edit")?;
                self.state.edit_task(thread, &id, &text).await?;
            }
            Action::Show => {}
        }
        self.state.thread_tasks(thread).await
    }
}

impl Tool for Tasks {
    fn name(&self) -> &'static str {
        "tasks"
    }

    fn description(&self, context: &ToolContext) -> String {
        if self.plan_form(context).is_some() {
            return text::DESCRIPTION.into();
        }
        "Your board for this work: write down the tasks you set yourself, each with its \
         steps, and mark them done as you go. Plan with it before work of several steps, look at it \
         after a pause or a summary of the conversation, and mark a step done as soon as it \
         is: the next step then starts, and the user sees how long each one took. A task is \
         done when all its steps are; mark it done yourself to close the rest. Actions: \
         add (task, items), add_items (id, items), show, start (id of a step to begin out \
         of order), done (id of a task or a step), edit (id, text). IDs may be given by the \
         last 8 characters the board shows. Every call returns the board."
            .into()
    }

    /// Both forms, so a call of either passes the check and its own parser decides.
    fn parameters(&self) -> Value {
        text::checked_schema(schema::of::<TasksArguments>())
    }

    fn offered_parameters(&self, context: &ToolContext) -> Value {
        match self.plan_form(context) {
            Some(_) => text::schema(),
            None => schema::of::<TasksArguments>(),
        }
    }

    /// `show` reads. `run`, `review`, `fail`, `reopen` and `stop` set a state that setting
    /// again does not change, and the plan's `add` finds its own tasks when run again; the
    /// board's `add` and every `edit` are left to the user after a restart.
    fn repeatable(&self, arguments: &Value) -> bool {
        match action(arguments) {
            "show" => true,
            "add" => arguments.get("tasks").is_some(),
            action => PLAN_ONLY.contains(&action),
        }
    }

    fn read_only(&self, arguments: &Value) -> bool {
        action(arguments) == "show"
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let name = action(&arguments).to_owned();
            if let Some(lead) = self.plan_form(&context)
                && !BOARD_ONLY.contains(&name.as_str())
            {
                let arguments = parse_arguments(arguments)?;
                let answer = lead
                    .call(&self.state, &self.thread_id, arguments, &context)
                    .await?;
                if answer.changed {
                    lead.team.changed(&self.thread_id).await?;
                    self.tell_clients(&context).await?;
                }
                return Ok(answer.text);
            }
            let plan_fields = ["tasks", "budget", "ids"];
            if PLAN_ONLY.contains(&name.as_str())
                || plan_fields
                    .iter()
                    .any(|field| arguments.get(field).is_some())
            {
                bail!(
                    "the plan's actions need TEAM mode; \
                     use add with task and items"
                );
            }
            let arguments = parse_arguments(arguments)?;
            let tasks = self.run(arguments).await?;
            if name != "show"
                && let Some(progress) = context.progress
            {
                progress.emit(ThreadEvent::Tasks(tasks.clone()));
            }
            Ok(board(&tasks))
        })
    }
}

impl Tasks {
    async fn tell_clients(&self, context: &ToolContext) -> anyhow::Result<()> {
        if let Some(progress) = &context.progress {
            let tasks = self.state.thread_tasks(&self.thread_id).await?;
            progress.emit(ThreadEvent::Tasks(tasks));
        }
        Ok(())
    }
}

/// The board as text: what is left first, then what is done.
pub fn board(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "The board is empty.".into();
    }
    let mut text = String::new();
    for (title, done) in [("To do", false), ("Done", true)] {
        let column: Vec<&Task> = tasks.iter().filter(|task| task.done == done).collect();
        if column.is_empty() {
            continue;
        }
        text.push_str(&format!("{title}\n"));
        for task in column {
            let finished = task.items.iter().filter(|item| item.done).count();
            text.push_str(&format!(
                "  {} {} #{} ({finished}/{})\n",
                mark(task.done),
                task.description,
                short(&task.task_id),
                task.items.len()
            ));
            for item in &task.items {
                let mark = match (item.done, item.started_at) {
                    (true, _) => "[x]",
                    (false, Some(_)) => "[>]",
                    (false, None) => "[ ]",
                };
                let took = match (item.started_at, item.finished_at) {
                    (Some(start), Some(end)) => format!(" took {}s", (end - start).max(0) / 1000),
                    _ => String::new(),
                };
                text.push_str(&format!(
                    "      {mark} {} #{}{took}\n",
                    item.description,
                    short(&item.item_id)
                ));
            }
        }
    }
    text
}

fn mark(done: bool) -> &'static str {
    if done { "[x]" } else { "[ ]" }
}

fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(SHORT_ID)..]
}

#[cfg(test)]
mod lead_tests;
#[cfg(test)]
mod tests;
