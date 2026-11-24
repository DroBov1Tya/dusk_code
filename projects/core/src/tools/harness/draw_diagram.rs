//! `draw_diagram`: a diagram in the chat, given as nodes and edges the chat lays out
//! itself. Like `show`, the call's arguments are the diagram: the thread stores them with
//! the assistant message, and a client draws it from them, live and after a reload. So
//! the tool only checks the diagram and answers with its plain text.

use serde_json::Value;

use crate::BoxFuture;
use crate::tools::{Tool, ToolContext, schema, schema::parse_arguments};

use dusk_protocol::thread::{Block, DIAGRAM_TOOL, Diagram};

pub struct DrawDiagram;

impl Tool for DrawDiagram {
    /// Keeps the chat's own records: no project file changes.
    fn writes_project(&self, _arguments: &Value) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        DIAGRAM_TOOL
    }

    // Only a chat has a user to show a diagram to.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Draw a diagram in the chat; it lays out the nodes and edges itself. Use it instead \
         of `show` or text for structure with many links: a flow, an architecture, a \
         sequence, a timeline, a tree or states; never ASCII art or Mermaid. Kinds: `flow` \
         (shape `diamond` decides, `pill` starts or ends), `architecture` (`groups` for \
         tiers, shape `store` for data), `sequence` (nodes are participants left to right, \
         edges the messages in order, `dashed` for replies), `timeline` (nodes are events \
         in order, each with `when`), `tree` (edges parent to child), `state` (shapes \
         `start` and `end`). One idea, about a dozen nodes, labels of a few words; a tone \
         only on the one or two nodes to see first. E.g. {\"kind\":\"flow\",\"nodes\":[{\"id\
         \":\"in\",\"label\":\"Parse\"},{\"id\":\"ok\",\"label\":\"Valid?\",\"shape\":\
         \"diamond\"}],\"edges\":[{\"from\":\"in\",\"to\":\"ok\"}]}"
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Diagram>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let diagram: Diagram = parse_arguments(arguments)?;
            Ok(Block::Diagram(diagram).text())
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
        tools::{Progress, ToolRegistry, ToolResult, ToolScope},
    };

    fn default_config() -> crate::config::Config {
        let dir = temp_dir("diagram_config");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        (*tool_context(&scope).config).clone()
    }

    async fn call(
        registry: &ToolRegistry,
        arguments: Value,
        config: crate::config::Config,
    ) -> (ToolCall, ToolResult) {
        let dir = temp_dir("diagram");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let call = ToolCall {
            id: "c".into(),
            name: DIAGRAM_TOOL.into(),
            arguments,
        };
        let (events, _kept) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let result = registry
            .call(&call, &scope, &Arc::new(config), progress)
            .await;
        (call, result)
    }

    fn registry() -> ToolRegistry {
        let mut registry = ToolRegistry::default();
        registry.set_depth(0);
        registry.chat = true;
        registry.add(Box::new(DrawDiagram));
        registry
    }

    /// Every client draws diagrams, so the tool is offered in every built-in mode by
    /// default; `tools: { draw_diagram: false }` switches it off and a call is refused.
    #[tokio::test]
    async fn it_is_on_by_default_and_the_config_switches_it_off() {
        use crate::catalog::modes::{MAIN, ORCHESTRATOR, PLAN, TEAM};
        let dir = temp_dir("diagram_switch");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let registry = registry();
        let offered = |config: &crate::config::Config| {
            let specs = registry.specs(&scope, &Arc::new(config.clone()));
            specs.iter().any(|spec| spec.name == DIAGRAM_TOOL)
        };
        let mut on = default_config();
        for mode in [MAIN, PLAN, ORCHESTRATOR, TEAM] {
            on.set_mode(mode.into());
            assert!(offered(&on), "{mode}");
        }

        let off = crate::config::Config::parse(b"tools: { draw_diagram: false }\n").unwrap();
        assert!(!offered(&off));
        let tree = json!({ "kind": "tree", "nodes": [{ "id": "a" }, { "id": "b" }],
                           "edges": [{ "from": "a", "to": "b" }] });
        let (call, result) = super::tests::call(&registry, tree, off).await;
        assert_eq!(result.content, "error: tool draw_diagram is switched off");
        assert_eq!(Block::shown(&call, &result.content), None);
    }

    #[tokio::test]
    async fn a_diagram_comes_back_as_its_text_and_is_read_from_the_stored_call() {
        let tree = json!({ "kind": "tree", "nodes": [{ "id": "a" }, { "id": "b" }],
                           "edges": [{ "from": "a", "to": "b" }] });
        let (call, result) = super::tests::call(&registry(), tree, default_config()).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.starts_with("Diagram:"), "{}", result.content);
        let stored: ToolCall =
            serde_json::from_str(&serde_json::to_string(&call).unwrap()).unwrap();
        let block = Block::shown(&stored, &result.content).unwrap();
        assert_eq!(block.text(), result.content);
    }

    #[tokio::test]
    async fn a_wrong_call_says_what_to_fix_and_shows_nothing() {
        for (arguments, said) in [
            (
                json!({ "kind": "flow", "nodes": [{ "id": "a" }],
                        "edges": [{ "from": "a", "to": "b" }] }),
                "\"b\", which is not given",
            ),
            (
                json!({ "kind": "flow", "nodes": [{ "id": "a", "svg": "<svg/>" }] }),
                "svg",
            ),
            (
                json!({ "diagram": { "kind": "flow", "nodes": [{ "id": "a" }] } }),
                "diagram",
            ),
        ] {
            let (call, result) = super::tests::call(&registry(), arguments, default_config()).await;
            assert!(result.is_error, "{}", result.content);
            assert!(result.content.contains(said), "{said}: {}", result.content);
            assert_eq!(Block::shown(&call, &result.content), None);
        }
    }

    /// The example the model learns from is a diagram the tool takes.
    #[test]
    fn the_description_example_is_a_valid_diagram() {
        let dir = temp_dir("diagram_example");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let description = DrawDiagram.description(&tool_context(&scope));
        let start = description.find("{\"kind\"").expect("an example");
        let example: Value = serde_json::from_str(&description[start..]).unwrap();
        let diagram: Diagram = serde_json::from_value(example).unwrap();
        assert!(Block::Diagram(diagram).text().starts_with("Diagram:"));
    }
}
