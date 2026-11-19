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
        steps: Vec::new(),
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

#[test]
fn one_retry_row_is_replaced_counts_down_and_goes_when_the_call_answers() {
    let retrying = |attempt| ThreadEvent::Retrying {
        attempt,
        error: "timeout".into(),
        kind: dusk_protocol::model::ProviderErrorKind::Overloaded,
        wait_ms: 8000,
        max_attempts: Some(3),
    };
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::TurnStarted(dusk_protocol::thread::Turn {
        turn_id: "t".into(),
        thread_id: "x".into(),
        last_sequence: 1,
        response_message_id: None,
        status: dusk_protocol::thread::TurnStatus::Running,
        started_at: 0,
        finished_at: None,
        error_message: None,
        waiting: None,
        steps: 0,
        tokens: 0,
        cost_micro_usd: 0,
    }));
    chat.apply(retrying(1));
    chat.apply(retrying(2));
    // No notice per attempt; the working row holds the newest.
    assert!(chat.items.is_empty());
    let line = |now| chat.work(now).expect("a row").label;
    assert!(
        line(0).ends_with("attempt 3 of 3 \u{00B7} next in 8 s"),
        "{}",
        line(0)
    );
    assert!(line(7_500).ends_with("next in 1 s"));
    assert!(line(8_000).ends_with("trying now"));

    chat.apply(ThreadEvent::Model(ModelEvent::Text("hi".into())));
    assert_eq!(chat.work(0).expect("a row").label, "Writing");
}

fn show(id: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: dusk_protocol::thread::SHOW_TOOL.into(),
        arguments,
    }
}

fn tool_message(id: &str, content: &str) -> Message {
    let mut message = message(MessageRole::Tool, content);
    message.tool_call_id = Some(id.into());
    message
}

/// The chat a `show` call and its result make, live and read back from history.
fn live_and_restored(call: ToolCall, result: &str) -> [ChatData; 2] {
    let mut assistant = message(MessageRole::Assistant, "Here is the plan");
    assistant.tool_calls.push(call.clone());
    let history = [
        message(MessageRole::User, "plan it"),
        assistant,
        tool_message(&call.id, result),
        message(MessageRole::Assistant, "Done"),
    ];
    let mut live = ChatData::default();
    live.apply(ThreadEvent::UserMessage(message(
        MessageRole::User,
        "plan it",
    )));
    live.apply(ThreadEvent::Model(ModelEvent::Text(
        "Here is the plan".into(),
    )));
    live.apply(ThreadEvent::ToolStarted(call.clone()));
    live.apply(ThreadEvent::ToolFinished {
        call_id: call.id,
        result: ToolResult::text(result.into()),
    });
    live.apply(ThreadEvent::Model(ModelEvent::Text("Done".into())));
    [ChatData::from_history(&history), live]
}

fn kinds(chat: &ChatData) -> Vec<&'static str> {
    chat.items
        .iter()
        .map(|entry| match &entry.item {
            Item::User(_) => "user",
            Item::Assistant { .. } => "answer",
            Item::Tool(_) => "tool",
            Item::Block(_) => "block",
            _ => "other",
        })
        .collect()
}

fn flow(title: &str, line: &str) -> serde_json::Value {
    serde_json::json!({ "flow": { "layers": [
        { "title": title, "cards": [{ "title": "card", "line": line }] },
        { "title": "next", "emphasis": true },
    ] } })
}

#[test]
fn a_shown_block_takes_its_steps_place_in_order_live_and_restored() {
    let checks = serde_json::json!({ "checks": { "items": [
        { "name": "unit tests", "state": "passed", "counts": { "passed": 2, "failed": 0, "total": 2 } },
    ] } });
    for arguments in [
        flow("Build", "compile"),
        checks,
        serde_json::json!({ "choices": { "labels": ["Yes", "No"] } }),
    ] {
        let call = show("s", arguments);
        let block = Block::asked(&call).expect("a block within caps");
        let chats = live_and_restored(call, &block.text());
        for chat in &chats {
            assert_eq!(kinds(chat), ["user", "answer", "block", "answer"]);
            assert!(matches!(&chat.items[2].item, Item::Block(shown) if *shown == block));
        }
        assert!(
            chats[0]
                .items
                .iter()
                .map(|e| &e.item)
                .eq(chats[1].items.iter().map(|e| &e.item))
        );
    }
}

#[test]
fn a_show_call_that_was_not_shown_stays_a_step() {
    let call = show("s", flow("Build", "compile"));
    for chat in live_and_restored(call, "the show tool is switched off") {
        assert_eq!(kinds(&chat), ["user", "answer", "tool", "answer"]);
        assert!(
            matches!(&chat.items[2].item, Item::Tool(tool) if tool.output == "the show tool is switched off")
        );
    }
    // Another tool whose answer happens to start with a block's heading stays a step too.
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::ToolStarted(call_with_flow_arguments()));
    chat.apply(ThreadEvent::ToolFinished {
        call_id: "b".into(),
        result: ToolResult::text("Flow:\nBuild".into()),
    });
    assert_eq!(kinds(&chat), ["tool"]);
}

fn call_with_flow_arguments() -> ToolCall {
    ToolCall {
        id: "b".into(),
        name: "bash".into(),
        arguments: flow("Build", "compile"),
    }
}

#[test]
fn hostile_block_content_stays_plain_text() {
    let script = "<script>alert(1)</script>";
    let link = "[open](javascript:alert(1))";
    let call = show("s", flow(script, link));
    let block = Block::asked(&call).expect("markup is plain text within caps");
    for chat in live_and_restored(call, &block.text()) {
        let Item::Block(Block::Flow(shown)) = &chat.items[2].item else {
            panic!("no flow");
        };
        let layer = &shown.layers()[0];
        assert_eq!(layer.title.as_str(), script);
        assert_eq!(
            layer.cards()[0].line.as_ref().map(|l| l.as_str()),
            Some(link)
        );
    }

    // Over a cap, several lines, or control characters: refused by the protocol, so the
    // call stays a step whose text is clipped, and nothing panics.
    let long = "x".repeat(100_000);
    for title in [long.as_str(), "one\ntwo", "bell\u{7}", "\u{202e}reversed"] {
        let call = show("s", flow(title, "line"));
        assert!(Block::asked(&call).is_none());
        for chat in live_and_restored(call, "Flow:\nwhatever the tool said") {
            assert_eq!(kinds(&chat), ["user", "answer", "tool", "answer"]);
            let Item::Tool(tool) = &chat.items[2].item else {
                panic!("no step");
            };
            assert!(tool.arguments.chars().count() <= dusk_protocol::thread::SHOWN_CHARS + 4);
        }
    }
}

fn change(id: i64, call_id: &str, undone: bool) -> dusk_protocol::catalog::SkillChange {
    dusk_protocol::catalog::SkillChange {
        id,
        skill: "git-release".into(),
        call_id: call_id.into(),
        action: dusk_protocol::catalog::SkillSaveAction::Updated,
        reason: "fix".into(),
        changed_at_ms: 0,
        undone,
        can_undo: !undone,
    }
}

#[test]
fn a_skill_change_replaces_its_row_by_id_and_hangs_on_its_step() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::ToolStarted(call("1")));
    chat.apply(ThreadEvent::SkillChanged(change(7, "1", false)));
    chat.apply(ThreadEvent::SkillChanged(change(8, "other", false)));
    chat.apply(ThreadEvent::SkillChanged(change(7, "1", true)));
    // A stale list read before the undo does not bring Undo back.
    chat.put_skill_change(change(7, "1", false));
    assert_eq!(chat.skill_changes.len(), 2);
    let rows = chat.skill_changes_of("1");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].undone && !rows[0].can_undo);
}
