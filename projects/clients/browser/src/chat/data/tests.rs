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

fn message(role: MessageRole, content: &str) -> Message {
    Message {
        message_id: String::new(),
        thread_id: String::new(),
        sequence: 1,
        role,
        content: content.into(),
        reasoning: None,
        tool_calls: Vec::new(),
        tool_call_id: None,
        artifacts: Vec::new(),
        from_thread_id: None,
        from_job_id: None,
        content_tokens: 0,
        tool_calls_tokens: 0,
        reasoning_tokens: 0,
        created_at: 0,
    }
}

#[test]
fn answers_and_job_results_are_distinct_in_history_and_live_events() {
    let question = Question {
        question: "Choose".into(),
        options: vec!["A".into(), "B".into()],
        multiple: false,
    };
    let ask = ToolCall {
        id: "ask".into(),
        name: "ask_user".into(),
        arguments: serde_json::to_value(&question).unwrap(),
    };
    let answer = question.answer(&[1], "");
    let job = dusk_protocol::thread::job_message("id", "failed", "<script>out</script>");
    let mut assistant = message(MessageRole::Assistant, "");
    assistant.tool_calls.push(ask.clone());
    let history = [
        assistant,
        message(MessageRole::User, &answer),
        message(MessageRole::Job, &job),
    ];
    let mut live = ChatData::default();
    live.apply(ThreadEvent::ToolStarted(ask));
    live.apply(ThreadEvent::UserMessage(message(
        MessageRole::User,
        &answer,
    )));
    live.apply(ThreadEvent::FromJob(message(MessageRole::Job, &job)));
    for chat in [ChatData::from_history(&history), live] {
        assert!(matches!(&chat.items[1].item, Item::Answered(answers) if answers[0].options[1].1));
        assert!(
            matches!(&chat.items[2].item, Item::Job(result) if result.failed() && result.output == "<script>out</script>")
        );
    }
    let fallback = ChatData::from_history(&[message(MessageRole::Job, "unrecognised")]);
    assert!(
        matches!(&fallback.items[0].item, Item::Job(result) if result.output == "unrecognised")
    );
}
