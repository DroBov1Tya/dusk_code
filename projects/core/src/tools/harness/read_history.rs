//! `read_history`: the thread's own messages, read by sequence or found by text. After a
//! compaction the summary stands in for older messages; this is how the agent gets
//! their details back.

use std::sync::Arc;

use anyhow::ensure;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{
    builtin::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{
    BoxFuture,
    state::{Message, MessageRole, State},
};

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;
/// Keeps one long message from filling the whole result.
const MAX_MESSAGE_CHARS: usize = 2000;

pub struct ReadHistory {
    state: Arc<State>,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadHistoryArguments {
    search: Option<String>,
    from: Option<u64>,
    limit: Option<u32>,
}

impl ReadHistory {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}

impl Tool for ReadHistory {
    fn name(&self) -> &'static str {
        "read_history"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Read earlier messages of this conversation, including those replaced by a summary. \
         Give `search` to find messages containing some text (at least 3 characters, best \
         matches first), or `from` to read in order from a message number. Each message \
         is shown with its number, so you can read on around a match."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "search": { "type": "string", "description": "Text to look for." },
                "from": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "First message number to read. Defaults to 1."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIMIT,
                    "description": "How many messages. Defaults to 20."
                }
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: ReadHistoryArguments = parse_arguments(arguments)?;
            let limit = arguments.limit.unwrap_or(DEFAULT_LIMIT);
            ensure!(
                (1..=MAX_LIMIT).contains(&limit),
                "limit must be between 1 and {MAX_LIMIT}"
            );
            let messages = match (&arguments.search, arguments.from) {
                (Some(_), Some(_)) => anyhow::bail!("give either search or from, not both"),
                (Some(text), None) => {
                    let found = self.state.search_messages(&self.thread_id, text, limit);
                    found.await?
                }
                (None, from) => {
                    let from = from.unwrap_or(1);
                    let read = self.state.messages_from(&self.thread_id, from, limit);
                    read.await?
                }
            };
            if messages.is_empty() {
                return Ok("no messages".into());
            }
            Ok(messages
                .iter()
                .map(describe)
                .collect::<Vec<_>>()
                .join("\n\n"))
        })
    }
}

fn describe(message: &Message) -> String {
    let author = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool result",
        MessageRole::Job => "background job result",
    };
    let mut text = format!(
        "#{} {author}:\n{}",
        message.sequence,
        clip(&message.content)
    );
    for call in &message.tool_calls {
        let arguments = clip(&call.arguments.to_string());
        text.push_str(&format!("\ncalls {} {arguments}", call.name));
    }
    text
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((end, _)) => format!("{}... [cut]", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        testing::{temp_dir, temp_state, tool_context},
        tools::ToolScope,
    };

    async fn history(name: &str) -> ReadHistory {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        for (uid, text) in [("1", "Deploy to staging"), ("2", "Fix src/main.rs please")] {
            state.add_user_message("thread", uid, text).await.unwrap();
        }
        let other = "main.rs of another thread";
        state.add_user_message("other", "3", other).await.unwrap();
        ReadHistory::new(Arc::new(state), "thread".into())
    }

    async fn read(tool: &ReadHistory, arguments: Value) -> anyhow::Result<String> {
        let directory = temp_dir("history_scope");
        std::fs::create_dir_all(&directory).unwrap();
        let scope = ToolScope::new(&directory, &directory).unwrap();
        tool.call(arguments, tool_context(&scope)).await
    }

    #[tokio::test]
    async fn finds_substrings_in_its_own_thread_only() {
        let tool = history("history_search").await;
        let found = read(&tool, json!({ "search": "main.rs" })).await.unwrap();
        assert_eq!(found, "#2 user:\nFix src/main.rs please");
        let found = read(&tool, json!({ "search": "STAGING" })).await.unwrap();
        assert_eq!(found, "#1 user:\nDeploy to staging");
    }

    #[tokio::test]
    async fn query_syntax_is_plain_text() {
        let tool = history("history_syntax").await;
        let found = read(&tool, json!({ "search": "\"main OR" })).await.unwrap();
        assert_eq!(found, "no messages");
        assert!(read(&tool, json!({ "search": "ma" })).await.is_err());
    }
}
