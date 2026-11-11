use crate::providers::ToolCall;
use serde_json::json;

use super::*;

#[test]
fn tools_called_before_a_mode_switch_are_named() {
    let call = |name: &str| ToolCall {
        id: name.into(),
        name: name.into(),
        arguments: json!({}),
    };
    let messages = [ChatMessage::Assistant {
        text: String::new(),
        tool_calls: vec![call("run_command"), call("ask_agent"), call("run_command")],
    }];
    let spec = |name: &str| ToolSpec {
        name: name.into(),
        description: String::new(),
        parameters: json!({}),
    };
    let note = missing_tools(&messages, &[spec("ask_agent")]).unwrap();
    assert!(note.contains(": run_command."), "{note}");
    assert!(missing_tools(&messages, &[spec("ask_agent"), spec("run_command")]).is_none());
}
