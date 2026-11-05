//! `name_session`: the model gives this session its name: one short, meaningful title,
//! from the content of the user's first message. It is the tool version of what the
//! runtime does after the first turn, so the name can come from the model at once.

use std::sync::Arc;

use anyhow::ensure;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    runtime::ThreadEvent,
    state::State,
    tools::{Tool, ToolContext, schema, schema::parse_arguments},
};

/// What the client accepts for a title, as the `Rename` request checks it.
const MAX_TITLE_CHARS: usize = 200;

pub struct NameSession {
    state: Arc<State>,
    thread_id: String,
}

/// Flat, since providers want one object at the top of a schema.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// The name for this session, from what the user's message is about.
    title: String,
}

impl NameSession {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}

impl Tool for NameSession {
    fn name(&self) -> &'static str {
        "name_session"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Name this session with a short, meaningful title taken from what the message is \
         about. Call it once, right after the first user message, based on its content. \
         One line, at most 200 characters."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let title = one_line(&arguments.title);
            ensure!(!title.is_empty(), "title is empty");
            ensure!(
                title.chars().count() <= MAX_TITLE_CHARS,
                "a title is at most {MAX_TITLE_CHARS} characters"
            );
            self.state.set_title(&self.thread_id, &title).await?;
            if let Some(progress) = &context.progress {
                progress.emit(ThreadEvent::TitleChanged {
                    title: title.clone(),
                });
            }
            Ok(format!("named: {title}"))
        })
    }
}

/// Whitespace collapses to single spaces, so the name stays one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        testing::{temp_dir, temp_state, tool_context},
        tools::ToolScope,
    };

    #[tokio::test]
    async fn a_call_names_the_thread() {
        let state = Arc::new(temp_state("name_session").await);
        state.create_thread("thread", "test").await.unwrap();
        let root = temp_dir("name_session_scope");
        std::fs::create_dir_all(&root).unwrap();
        let scope = ToolScope::new(&root, &root).unwrap();
        let tool = NameSession::new(Arc::clone(&state), "thread".into());
        let answer = tool
            .call(
                json!({"title": "fix the  flaky\nbuild"}),
                tool_context(&scope),
            )
            .await
            .unwrap();
        assert_eq!(answer, "named: fix the flaky build");
        let thread = state.load_thread("thread").await.unwrap().unwrap();
        assert_eq!(thread.title.as_deref(), Some("fix the flaky build"));
    }
}
