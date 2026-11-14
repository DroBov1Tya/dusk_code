use super::*;
use crate::{model::ToolCall, thread::ThreadEvent, tools::ToolResult};

fn call(arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "1".into(),
        name: "bash".into(),
        arguments,
    }
}

#[test]
fn a_step_shows_the_first_line_of_what_it_was_asked() {
    let tool = Tool::new(
        &call(serde_json::json!({ "command": "ls\nmore" })),
        ToolState::Running,
    );
    assert_eq!(tool.detail, "ls");
    let tool = Tool::new(&call(serde_json::json!({ "x": 1 })), ToolState::Done);
    assert_eq!(tool.detail, "");
    let long = "y".repeat(500);
    assert_eq!(tool_detail(&serde_json::json!({ "path": long })).len(), 160);
}

#[test]
fn a_result_lands_on_its_step_and_long_text_is_clipped() {
    let mut tool = Tool::new(&call(serde_json::json!({})), ToolState::Running);
    tool.finish(&ToolResult::text("out".into()));
    assert_eq!((tool.state, tool.output.as_str()), (ToolState::Done, "out"));
    let mut failed = ToolResult::text("boom".into());
    failed.is_error = true;
    tool.finish(&failed);
    assert_eq!(tool.state, ToolState::Failed);
    let long = "\u{e9}".repeat(SHOWN_CHARS + 1);
    let clipped = clip(&long);
    assert!(clipped.ends_with("\n..."));
    assert_eq!(clipped.chars().count(), SHOWN_CHARS + 4);
    assert_eq!(clip("short"), "short");
}

#[test]
fn events_leave_their_lines_and_messages_leave_none() {
    assert_eq!(
        note(&ThreadEvent::Failed("x".into())),
        Some(Note::Error("x".into()))
    );
    assert_eq!(
        note(&ThreadEvent::ArbiterDecided {
            what: "rm".into(),
            allow: false,
            reason: "no".into()
        }),
        Some(Note::Notice("Arbiter denied rm: no".into()))
    );
    assert_eq!(note(&ThreadEvent::Idle), None);
}
