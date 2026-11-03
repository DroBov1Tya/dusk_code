//! `tasks`: the agent's own board for the work of a thread. It writes down the tasks it
//! set itself with their steps, looks at the board, rewrites a task or a step, and marks
//! a step or a whole task done. Steps are timed as they run. The board lives in the
//! database, so it outlives a compaction.

use std::sync::Arc;

use anyhow::{Context, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{Tool, ToolContext, schema, schema::parse_arguments};
use crate::{
    BoxFuture,
    runtime::ThreadEvent,
    state::{State, Task},
};

/// IDs are shown by their last characters, which are random; the agent may use those.
const SHORT_ID: usize = 8;

pub struct Tasks {
    state: Arc<State>,
    thread_id: String,
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
        Self { state, thread_id }
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

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Your board for this work: write down the tasks you set yourself, each with its \
         steps, and mark them done as you go. Plan with it before longer work, look at it \
         after a pause or a summary of the conversation, and mark a step done as soon as it \
         is: the next step then starts, and the user sees how long each one took. A task is \
         done when all its steps are; mark it done yourself to close the rest. Actions: \
         add (task, items), add_items (id, items), show, start (id of a step to begin out \
         of order), done (id of a task or a step), edit (id, text). IDs may be given by the \
         last 8 characters the board shows. Every call returns the board."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<TasksArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments = parse_arguments(arguments)?;
            let tasks = self.run(arguments).await?;
            if let Some(progress) = context.progress {
                progress.emit(ThreadEvent::Tasks(tasks.clone()));
            }
            Ok(board(&tasks))
        })
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn steps_may_be_text_or_objects() {
        let add =
            json!({ "action": "add", "task": "port", "items": ["read", { "text": "write" }] });
        let add: TasksArguments = parse_arguments(add).unwrap();
        let items: Vec<String> = add.items.into_iter().map(Step::into_text).collect();
        assert_eq!(items, ["read", "write"]);
        let schema = schema::of::<TasksArguments>();
        let check = schema::Check::new(&schema);
        let objects = json!({ "action": "add", "task": "port", "items": [{ "text": "read" }] });
        assert!(check.problems("tasks", &schema, &objects).is_none());
        let wrong = json!({ "action": "drop", "id": "a1b2c3d4", "force": true });
        assert!(check.problems("tasks", &schema, &wrong).is_some());
    }
}
