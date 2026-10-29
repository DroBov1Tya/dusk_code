//! `tasks`: the agent's own board for the work of a thread. It writes down the tasks it
//! set itself with their steps, looks at the board, and marks a step or a whole task
//! done. The board lives in the database, so it outlives a compaction.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext, builtin::parse_arguments};
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

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum TasksArguments {
    /// A new task with its steps.
    Add {
        task: String,
        #[serde(default)]
        items: Vec<String>,
    },
    /// More steps for a task.
    AddItems {
        id: String,
        items: Vec<String>,
    },
    Show,
    /// A step, or a whole task with all its steps.
    Done {
        id: String,
    },
}

impl Tasks {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }

    async fn run(&self, arguments: TasksArguments) -> anyhow::Result<Vec<Task>> {
        let thread = &self.thread_id;
        match arguments {
            TasksArguments::Add { task, items } => {
                self.state.add_task(thread, &task, &items).await?;
            }
            TasksArguments::AddItems { id, items } => {
                self.state.add_task_items(thread, &id, &items).await?;
            }
            TasksArguments::Done { id } => {
                self.state.complete(thread, &id).await?;
            }
            TasksArguments::Show => {}
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
         is. Mark a task done when the whole of it is, which marks its steps done too. \
         Actions: add (task, items), add_items (id, items), show, done (id of a task or a \
         step). IDs may be given by the last 8 characters the board shows. Every call \
         returns the board."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["add", "add_items", "show", "done"] },
                "task": { "type": "string", "description": "For add: the task in a few words." },
                "items": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "For add and add_items: the steps, in order."
                },
                "id": {
                    "type": "string",
                    "description": "For add_items: a task. For done: a task or a step."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
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
                text.push_str(&format!(
                    "      {} {} #{}\n",
                    mark(item.done),
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
    fn arguments_are_strict() {
        let show: TasksArguments = parse_arguments(json!({ "action": "show" })).unwrap();
        assert!(matches!(show, TasksArguments::Show));
        let add = json!({ "action": "add", "task": "port", "items": ["read"] });
        assert!(matches!(
            parse_arguments::<TasksArguments>(add).unwrap(),
            TasksArguments::Add { .. }
        ));
        for wrong in [
            json!({ "action": "done" }),
            json!({ "action": "drop", "id": "a1b2c3d4" }),
            json!({ "action": "done", "id": "a1b2c3d4", "force": true }),
        ] {
            assert!(parse_arguments::<TasksArguments>(wrong).is_err());
        }
    }
}
