use super::*;
use dusk_protocol::model::ToolCall;
use dusk_protocol::tools::ToolResult;

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "bash".into(),
        arguments: serde_json::json!({ "command": "ls\nmore" }),
    }
}

#[test]
fn streamed_text_joins_one_answer_and_a_tool_splits_it() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::Model(ModelEvent::Text("Hel".into())));
    chat.apply(ThreadEvent::Model(ModelEvent::Text("lo".into())));
    chat.apply(ThreadEvent::ToolStarted(call("1")));
    chat.apply(ThreadEvent::Model(ModelEvent::Text("Done".into())));
    let kinds: Vec<&str> = chat
        .items
        .iter()
        .map(|entry| match &entry.item {
            Item::Assistant { .. } => "answer",
            Item::Tool(_) => "tool",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["answer", "tool", "answer"]);
    assert!(matches!(&chat.items[0].item, Item::Assistant { text, .. } if text == "Hello"));
}

#[test]
fn a_tool_result_lands_on_its_call() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::ToolStarted(call("1")));
    chat.apply(ThreadEvent::ToolFinished {
        call_id: "1".into(),
        result: ToolResult::text("out".into()),
    });
    let Item::Tool(tool) = &chat.items[0].item else {
        panic!("no tool");
    };
    assert_eq!((tool.state, tool.output.as_str()), (ToolState::Done, "out"));
    assert_eq!(tool.detail, "ls");
}

#[test]
fn an_approval_shows_until_it_is_closed() {
    let mut chat = ChatData::default();
    let request = ApprovalRequest {
        id: 4,
        request: dusk_protocol::tools::Request::Command {
            command: "rm x".into(),
        },
        session: None,
    };
    chat.apply(ThreadEvent::Approval(request));
    assert_eq!(chat.approvals.len(), 1);
    chat.apply(ThreadEvent::ApprovalClosed { id: 4 });
    assert!(chat.approvals.is_empty());
}

#[test]
fn a_subagents_events_go_to_its_own_chat() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::SubagentStarted {
        call_id: "c".into(),
        thread_id: "sub".into(),
        agent: "general".into(),
        task: "look".into(),
        effort: None,
        model: None,
    });
    chat.apply(ThreadEvent::Subagent {
        thread_id: "sub".into(),
        event: Box::new(ThreadEvent::Model(ModelEvent::Text("hi".into()))),
    });
    assert!(chat.items.is_empty());
    assert_eq!(chat.agent_chats["sub"].items.len(), 1);
    assert!(matches!(chat.agents[0].state, AgentState::Writing));
}
