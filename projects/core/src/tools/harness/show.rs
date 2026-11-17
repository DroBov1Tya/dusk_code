//! `show`: a structured block in the chat, such as a flow of layers, a list of checks
//! or reply buttons. The call's arguments are the block: the thread stores them with the
//! assistant message, and a client draws the block from them, live and after a reload.
//! So the tool only checks the block and answers with its plain text.

use serde_json::Value;

use crate::BoxFuture;
use crate::tools::{Tool, ToolContext, schema, schema::parse_arguments};

use dusk_protocol::thread::{Block, MAX_CHOICES, MIN_CHOICES, SHOW_TOOL, Show as Arguments};

pub struct Show;

impl Tool for Show {
    fn name(&self) -> &'static str {
        SHOW_TOOL
    }

    // Only a chat has a user to show a block to.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Show the user a block the chat draws, instead of drawing it in text. Give \
             exactly one: `flow`, layers top to bottom joined by arrows, each with optional \
             cards, for an architecture or a pipeline; `checks`, test and CI results by \
             state; `choices`, {MIN_CHOICES} to {MAX_CHOICES} reply buttons under your \
             answer: call it last and end your turn, a click comes as the user's next \
             message. Every text in a block is one plain line. In your own text, \
             `{{{{success: Passed}}}}` draws a coloured label; the tones are neutral, info, \
             success, warning, danger and accent."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let block: Block = parse_arguments(arguments)?;
            Ok(block.text())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dusk_protocol::thread::{MAX_LAYERS, Tone};
    use serde_json::json;

    use super::*;
    use crate::{
        providers::ToolCall,
        testing::{temp_dir, tool_context},
        tools::{Progress, ToolRegistry, ToolScope},
    };

    /// The config of a user who switched `show` on.
    fn switched_on() -> crate::config::Config {
        let dir = temp_dir("show_config");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let mut config = (*tool_context(&scope).config).clone();
        config.set_tool_enabled(SHOW_TOOL, true);
        config
    }

    /// Until the clients draw blocks the tool is off: the model is not offered it, a call
    /// is refused, and no mode turns it on by its own defaults. So no request carries its
    /// description, the only place the label syntax is taught.
    #[tokio::test]
    async fn it_is_off_until_the_config_switches_it_on() {
        use crate::catalog::modes::{ORCHESTRATOR, PLAN};
        let dir = temp_dir("show_switch");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let mut registry = ToolRegistry::default();
        registry.set_depth(0);
        registry.chat = true;
        registry.add(Box::new(Show));
        let offered = |config: &crate::config::Config| {
            let specs = registry.specs(&scope, &Arc::new(config.clone()));
            specs.iter().any(|spec| spec.name == SHOW_TOOL)
        };
        let mut off = (*tool_context(&scope).config).clone();
        let mut on = switched_on();
        for mode in ["MAIN", PLAN, ORCHESTRATOR] {
            off.set_mode(mode.into());
            on.set_mode(mode.into());
            assert!(!offered(&off), "{mode}");
            assert!(offered(&on), "{mode}");
        }
        // A mode's own switch is not enough without the one in `tools:`.
        off.set_mode_tool_enabled(PLAN, SHOW_TOOL, true);
        off.set_mode(PLAN.into());
        assert!(!offered(&off));

        let call = ToolCall {
            id: "c".into(),
            name: SHOW_TOOL.into(),
            arguments: json!({ "choices": { "labels": ["a", "b"] } }),
        };
        let (events, _kept) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let config = tool_context(&scope).config;
        let result = registry.call(&call, &scope, &config, progress).await;
        assert_eq!(result.content, "error: tool show is switched off");
        assert_eq!(Block::shown(&call, &result.content), None);
    }

    /// A call as the thread makes it: through the registry, schema check first.
    async fn show(arguments: Value) -> (ToolCall, crate::tools::ToolResult) {
        let dir = temp_dir("show");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let config = Arc::new(switched_on());
        let mut registry = ToolRegistry::default();
        registry.set_depth(0);
        registry.add(Box::new(Show));
        let call = ToolCall {
            id: "c".into(),
            name: SHOW_TOOL.into(),
            arguments,
        };
        let (events, _kept) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let result = registry.call(&call, &scope, &config, progress).await;
        (call, result)
    }

    #[tokio::test]
    async fn a_block_comes_back_as_its_text_and_is_read_from_the_stored_call() {
        let arguments = json!({ "checks": { "items": [
            { "name": "cargo test", "state": "passed",
              "counts": { "passed": 4, "failed": 0, "total": 4 } },
        ] } });
        let (call, result) = show(arguments).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(
            result.content,
            "Checks:\npassed: cargo test (4 passed, 0 failed, 4 total)"
        );
        // What a client does with the stored call and its result after a reload.
        let stored: ToolCall =
            serde_json::from_str(&serde_json::to_string(&call).unwrap()).unwrap();
        let block = Block::shown(&stored, &result.content).unwrap();
        assert_eq!(block.text(), result.content);
    }

    #[tokio::test]
    async fn a_wrong_call_says_what_to_fix_and_shows_nothing() {
        let layer = json!({ "title": "a" });
        let long = "x".repeat(25);
        for (arguments, said) in [
            (json!({}), "exactly one of flow, checks and choices"),
            (
                json!({ "flow": { "layers": vec![layer.clone(); MAX_LAYERS + 1] } }),
                "/flow/layers",
            ),
            (
                json!({ "flow": { "layers": [layer.clone(), { "title": "b", "color": "#f00" }] } }),
                "color",
            ),
            (
                json!({ "choices": { "labels": ["ok", long] } }),
                "/choices/labels/1",
            ),
            (
                json!({ "choices": { "labels": ["a\nb", "c"] } }),
                "must be one line",
            ),
            (
                json!({ "checks": { "items": [{ "name": "t", "state": "green" }] } }),
                "/checks/items/0/state",
            ),
            (
                json!({ "choices": { "labels": ["a", "b"] }, "html": "<b>" }),
                "html",
            ),
        ] {
            let (call, result) = show(arguments).await;
            assert!(result.is_error, "{}", result.content);
            assert!(result.content.contains(said), "{said}: {}", result.content);
            assert_eq!(Block::shown(&call, &result.content), None);
        }
    }

    #[test]
    fn the_description_names_every_tone() {
        let dir = temp_dir("show_tones");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let description = Show.description(&tool_context(&scope));
        for tone in Tone::ALL {
            assert!(description.contains(tone.name()), "{}", tone.name());
        }
        assert!(description.contains("{{success: Passed}}"));
    }
}
