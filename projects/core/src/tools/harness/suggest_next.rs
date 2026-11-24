//! `suggest_next`: the model proposes the user's likely next message at the end of a
//! turn. The client shows it greyed in the empty message box and Tab takes it in; it is
//! never sent on its own. Only a chat has the tool, so a subagent never suggests.

use anyhow::{Context, ensure};
use serde_json::Value;

use crate::catalog::history::plain_line;
use crate::tools::{Tool, ToolContext, schema, schema::parse_arguments};
use crate::{BoxFuture, runtime::ThreadEvent};

use dusk_protocol::thread::{SUGGEST_NEXT_TOOL, SUGGESTION_CHARS, SuggestNext as Arguments};

pub struct SuggestNext;

/// The suggestion as the user is shown it: one plain line, control and direction marks
/// out, trimmed and capped. Empty when nothing is left.
fn suggestion_text(text: &str) -> String {
    plain_line(&text.replace(['\n', '\r'], " "), SUGGESTION_CHARS)
}

impl Tool for SuggestNext {
    /// Keeps the chat's own records or speaks to someone: no project file changes.
    fn writes_project(&self, _arguments: &Value) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        SUGGEST_NEXT_TOOL
    }

    // Only a chat has a user to suggest to.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Propose the user's likely next message; their message box shows it greyed and \
             Tab fills it in, so it is never sent without them. Call it at most once, as \
             the last call of a turn, together with your final answer, and only when the \
             next step is obvious: a yes to a question you asked, \"run the tests\", \
             \"commit it\". Write it as the user would, in their language, as one short \
             line (at most {SUGGESTION_CHARS} characters). Never invent a task; when no \
             next step is clear, do not call it. After it, end your turn without writing \
             more."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let Arguments { text } = parse_arguments(arguments)?;
            let text = suggestion_text(&text);
            ensure!(!text.is_empty(), "the suggestion is empty");
            let progress = context
                .progress
                .context("there is no user to suggest to in this thread")?;
            progress.emit(ThreadEvent::NextSuggested { text });
            Ok("Suggested. End your turn now without writing more.".into())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::{
        providers::ToolCall,
        testing::{temp_dir, tool_context},
        tools::{Progress, ToolRegistry, ToolScope},
    };

    /// A call as a thread at `depth` makes it, with the events it emitted.
    async fn suggest(depth: usize, arguments: Value) -> (crate::tools::ToolResult, Vec<String>) {
        let dir = temp_dir("suggest_next");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let config = Arc::clone(&tool_context(&scope).config);
        let mut registry = ToolRegistry::default();
        registry.set_depth(depth);
        registry.chat = depth == 0;
        registry.add(Box::new(SuggestNext));
        let call = ToolCall {
            id: "c".into(),
            name: SUGGEST_NEXT_TOOL.into(),
            arguments,
        };
        let (events, mut heard) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let result = registry.call(&call, &scope, &config, progress).await;
        let mut said = Vec::new();
        while let Ok(event) = heard.try_recv() {
            if let ThreadEvent::NextSuggested { text } = event {
                said.push(text);
            }
        }
        (result, said)
    }

    #[tokio::test]
    async fn a_suggestion_reaches_the_chat_as_one_clean_capped_line() {
        let (result, said) = suggest(0, json!({ "text": "  Да, встраивай все  " })).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(said, ["Да, встраивай все"]);

        let long = format!("run\u{202e} the\u{1b} tests\nnow {}", "x".repeat(500));
        let (result, said) = suggest(0, json!({ "text": long })).await;
        assert!(!result.is_error, "{}", result.content);
        let [text] = said.as_slice() else {
            panic!("{said:?}")
        };
        assert!(text.starts_with("run the tests now x"), "{text}");
        assert_eq!(text.chars().count(), SUGGESTION_CHARS);
        assert!(!text.contains(['\u{202e}', '\u{1b}', '\n']));
    }

    #[tokio::test]
    async fn an_empty_or_wrong_call_is_refused_and_shows_nothing() {
        for arguments in [
            json!({ "text": " \u{202e}\n " }),
            json!({}),
            json!({ "text": "yes", "send": true }),
        ] {
            let (result, said) = suggest(0, arguments.clone()).await;
            assert!(result.is_error, "{arguments}: {}", result.content);
            assert!(said.is_empty());
        }
    }

    /// A subagent has no user: it is not offered the tool, and a call is refused.
    #[tokio::test]
    async fn a_subagent_never_suggests() {
        let (result, said) = suggest(1, json!({ "text": "commit it" })).await;
        assert!(result.is_error, "{}", result.content);
        assert!(said.is_empty());
    }
}
