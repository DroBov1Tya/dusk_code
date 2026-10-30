//! `message_parent`: an agent writes to the thread that asked it while it works; the
//! message waits in the database until the asker's next step.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BoxFuture,
    runtime::system::Launcher,
    tools::{Tool, ToolContext, ToolKind},
};

pub struct MessageParent {
    launcher: Launcher,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageParentArguments {
    text: String,
}

impl MessageParent {
    pub fn new(launcher: Launcher, thread_id: String) -> Self {
        Self {
            launcher,
            thread_id,
        }
    }

    async fn send(&self, text: &str, cwd: &Path) -> Result<String> {
        ensure!(!text.trim().is_empty(), "text is empty");
        let thread = self.launcher.state.load_thread(&self.thread_id).await?;
        let parent = thread
            .and_then(|thread| thread.parent_thread_id)
            .context("this thread has no thread above it")?;
        self.launcher
            .deliver(&parent, &self.thread_id, text, cwd)
            .await?;
        Ok("sent".into())
    }
}

impl Tool for MessageParent {
    fn name(&self) -> &'static str {
        "message_parent"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Agents
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Write to the agent that gave you your task, while you work: a question, a finding \
         it should know now, how far you are. It reads the message between its steps. Your \
         final answer reaches it on its own, so do not send that too."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The message." }
            },
            "required": ["text"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: MessageParentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.send(&arguments.text, &cwd).await
        })
    }
}
