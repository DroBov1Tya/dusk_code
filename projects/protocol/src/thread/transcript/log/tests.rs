//! The rules of a chat, with the cases where the three clients' own copies disagreed
//! pinned to the behaviour each now shares.

use super::*;
use crate::catalog::SkillSaveAction;
use crate::plugin::{Kind, Listing, SUGGEST_TOOL, Suggested, Suggestion};
use crate::thread::{
    FAILED_PREFIXES, Message, MessageRole, SHOW_TOOL, SUGGEST_NEXT_TOOL, Turn, TurnStatus,
    stored_failure,
};
use crate::tools::ToolResult;
use serde_json::json;

type Chat = ChatLog;

fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}

fn message(role: MessageRole, content: &str) -> Message {
    Message {
        message_id: format!("m-{content}"),
        thread_id: "t".into(),
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

fn calling(calls: Vec<ToolCall>) -> Message {
    Message {
        tool_calls: calls,
        ..message(MessageRole::Assistant, "")
    }
}

fn result(call_id: &str, content: &str) -> Message {
    Message {
        tool_call_id: Some(call_id.into()),
        ..message(MessageRole::Tool, content)
    }
}

fn turn() -> Turn {
    Turn {
        turn_id: "turn".into(),
        thread_id: "t".into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: 0,
        finished_at: None,
        error_message: None,
        waiting: None,
        steps: 0,
        tokens: 0,
        cost_micro_usd: 0,
    }
}

fn text(piece: &str) -> ThreadEvent {
    ThreadEvent::Model(ModelEvent::Text(piece.into()))
}

fn tool_states(chat: &Chat) -> Vec<ToolState> {
    chat.items
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool) => Some(tool.state),
            _ => None,
        })
        .collect()
}

/// Browser and desktop read every stored call as done; the terminal read the core's
/// failure prefixes. A reopened chat now shows failed and stopped calls as failed.
#[test]
fn a_stored_call_that_failed_or_never_finished_reads_as_failed() {
    let calls = ["a", "b", "c", "d", "e"]
        .map(|id| call(id, "run_command", json!({ "command": id })))
        .to_vec();
    let history = [
        message(MessageRole::User, "go"),
        calling(calls),
        result("a", "fine"),
        result("b", "error: denied"),
        result("c", "stopped: the user interrupted the turn"),
        result("d", "not run: the user interrupted the turn"),
        result("e", "no result: the turn ended before this call finished"),
    ];
    let chat = Chat::from_history(&history);
    use ToolState::{Done, Failed};
    assert_eq!(tool_states(&chat), [Done, Failed, Failed, Failed, Failed]);
    assert!(FAILED_PREFIXES.iter().all(|prefix| stored_failure(prefix)));
}

/// The terminal left a turn's unfinished calls running forever in two of the three
/// clients; they now stop with the turn, as the core stores them.
#[test]
fn a_turn_that_ends_stops_the_calls_still_running() {
    let mut chat = Chat::default();
    chat.apply(ThreadEvent::TurnStarted(turn()), 1);
    chat.apply(ThreadEvent::ToolStarted(call("a", "bash", json!({}))), 1);
    chat.apply(ThreadEvent::TurnFinished(turn()), 2);
    assert_eq!(tool_states(&chat), [ToolState::Failed]);
    assert!(matches!(&chat.items[0], Item::Tool(tool) if tool.output == UNFINISHED));
}

/// The desktop's rules, which the browser and the terminal lacked: several tools at once
/// hand over to the one still running, text while a tool runs changes nothing, and an
/// agent that ended is not revived by what trails in, only by a new turn.
#[test]
fn a_subagent_follows_its_running_tools_and_stays_ended() {
    let mut chat = Chat::default();
    chat.apply(
        ThreadEvent::SubagentStarted {
            call_id: "ask".into(),
            thread_id: "sub".into(),
            agent: "general".into(),
            task: "look".into(),
            effort: None,
            model: None,
        },
        1,
    );
    let sub = |event: ThreadEvent| ThreadEvent::Subagent {
        thread_id: "sub".into(),
        event: Box::new(event),
    };
    let state = |chat: &Chat| chat.agent("sub").map(|agent| agent.state.clone());
    chat.apply(
        sub(ThreadEvent::ToolStarted(call("1", "read", json!({})))),
        2,
    );
    chat.apply(
        sub(ThreadEvent::ToolStarted(call("2", "grep", json!({})))),
        2,
    );
    chat.apply(
        sub(ThreadEvent::ToolFinished {
            call_id: "2".into(),
            result: ToolResult::text("x".into()),
        }),
        3,
    );
    assert_eq!(state(&chat), Some(AgentState::Tool("read".into())));
    // Text over a running tool changes nothing, and says so.
    assert_eq!(
        chat.apply(sub(text("hm")), 3),
        Applied::Subagent("sub".into())
    );
    assert_eq!(state(&chat), Some(AgentState::Tool("read".into())));
    chat.apply(sub(ThreadEvent::Failed("gone".into())), 4);
    assert_eq!(state(&chat), Some(AgentState::Failed));
    assert_eq!(chat.agent("sub").and_then(|agent| agent.ended_at), Some(4));
    chat.apply(sub(text("late")), 5);
    assert_eq!(state(&chat), Some(AgentState::Failed));
    chat.apply(sub(ThreadEvent::TurnStarted(turn())), 6);
    assert_eq!(state(&chat), Some(AgentState::Thinking));
    // Its own chat followed every event.
    assert!(chat.agent_chats["sub"].items.len() >= 2);
}

/// Browser and terminal showed no detail for a skill loaded by its name.
#[test]
fn a_skill_load_shows_the_skill_name_as_its_detail() {
    let tool = Tool::new(
        &call("a", "load_skill", json!({ "name": "notes/outline" })),
        ToolState::Running,
    );
    assert_eq!(tool.detail, "notes/outline");
    let tool = Tool::new(
        &call("a", "x", json!({ "name": "n", "path": "p" })),
        ToolState::Running,
    );
    assert_eq!(tool.detail, "p");
}

/// Only the desktop drew turn boundaries from history and kept the stored answer to
/// `ask_user` in the turn that asked.
#[test]
fn an_answer_to_the_agents_question_goes_on_with_its_turn() {
    let ask = call(
        "q",
        "ask_user",
        json!({ "question": "Which?", "options": ["A", "B"] }),
    );
    let history = [
        message(MessageRole::User, "go"),
        calling(vec![ask]),
        result("q", "asked"),
        message(MessageRole::User, "QUESTION: Which?\nANSWER: A"),
        message(MessageRole::Assistant, "done"),
        message(MessageRole::User, "next"),
    ];
    let chat = Chat::from_history(&history);
    // A boundary after "go" opens its turn, one after "done" ends it, none comes with
    // the answer, and "next" opens the next turn.
    assert_eq!(chat.activity_boundaries, [1, 4, 5]);
    assert!(matches!(&chat.items[2], Item::User { answered, .. } if answered.len() == 1));
    assert_eq!(chat.message_id(3), Some("m-done"));
    assert_eq!(chat.last_sent(), Some("next"));
}

/// The browser put every picture in the chat; a picture the model took to see is now
/// marked aside, which the desktop keeps in its right column.
#[test]
fn a_picture_the_model_took_to_see_is_aside() {
    let card = Card {
        id: "p".into(),
        ..Card::default()
    };
    let mut chat = Chat::default();
    for (id, name) in [("a", "display_image"), ("b", "screenshot")] {
        chat.apply(ThreadEvent::ToolStarted(call(id, name, json!({}))), 0);
        let mut done = ToolResult::text("ok".into());
        done.artifacts = vec![card.clone()];
        chat.apply(
            ThreadEvent::ToolFinished {
                call_id: id.into(),
                result: done,
            },
            0,
        );
    }
    let asides: Vec<bool> = chat
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Image { aside, .. } => Some(*aside),
            _ => None,
        })
        .collect();
    assert_eq!(asides, [false, true]);
    assert_eq!(chat.screenshots().count(), 1);
}

/// Only the desktop cut the chat when the core confirmed an /undo.
#[test]
fn an_undo_cuts_the_chat_where_it_was_asked() {
    let history = [
        message(MessageRole::User, "one"),
        message(MessageRole::Assistant, "first"),
        message(MessageRole::User, "two"),
        message(MessageRole::Assistant, "second"),
    ];
    let mut chat = Chat::from_history(&history);
    chat.rollback_at = Some(2);
    chat.apply(ThreadEvent::Undone { text: "two".into() }, 1);
    assert_eq!(chat.items.len(), 2);
    assert_eq!(chat.activity_boundaries, [1, 2]);
    assert_eq!(chat.offered.as_deref(), Some("two"));
    assert_eq!(chat.item_of("m-second"), None);
}

/// The terminal replaced a change wholesale, so a stale list brought Undo back there.
#[test]
fn an_undone_skill_change_stays_undone() {
    let change = |undone: bool| SkillChange {
        id: 1,
        skill: "notes".into(),
        call_id: "c".into(),
        action: SkillSaveAction::Updated,
        reason: "why".into(),
        changed_at_ms: 0,
        undone,
        can_undo: !undone,
    };
    let mut chat = Chat::default();
    chat.apply(ThreadEvent::SkillChanged(change(true)), 0);
    chat.skills_loaded(vec![change(false)]);
    assert_eq!(chat.skill_changes.len(), 1);
    assert!(chat.skill_changes[0].undone && !chat.skill_changes[0].can_undo);
}

/// The browser joined the answer of a new turn to the last answer of the turn before.
#[test]
fn a_new_turn_writes_a_new_answer() {
    let mut chat = Chat::default();
    chat.apply(ThreadEvent::TurnStarted(turn()), 0);
    chat.apply(text("one"), 0);
    chat.apply(ThreadEvent::TurnFinished(turn()), 0);
    chat.apply(ThreadEvent::TurnStarted(turn()), 0);
    chat.apply(text("two"), 0);
    let answers: Vec<&str> = chat
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Assistant { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(answers, ["one", "two"]);
}

/// A streamed piece changes one item and moves no row, until the answer stops being
/// blank; a client redraws no more than that.
#[test]
fn a_streamed_piece_changes_only_its_answer() {
    let mut chat = Chat::default();
    chat.apply(ThreadEvent::TurnStarted(turn()), 0);
    assert_eq!(chat.apply(text("  "), 0), Applied::Any);
    let shape = chat.shape();
    let revision = chat.revision(0);
    assert_eq!(chat.apply(text(" "), 0), Applied::Item(0));
    assert_eq!(chat.shape(), shape);
    assert_ne!(chat.revision(0), revision);
    assert_eq!(chat.apply(text("hi"), 0), Applied::Any);
    assert_ne!(chat.shape(), shape);
    assert_eq!(chat.apply(text("!"), 0), Applied::Item(0));
    assert_eq!(chat.take_changed(), Some(0));
    assert_eq!(chat.index_of(chat.id(0)), Some(0));
}

/// A shown block takes its step's place, live and read back, in the same order.
#[test]
fn a_shown_block_takes_its_steps_place_live_and_restored() {
    let show = call(
        "s",
        SHOW_TOOL,
        json!({ "choices": { "labels": ["Yes", "No"] } }),
    );
    let shown = Block::asked(&show).expect("a block").text();
    let read = call("r", "read_file", json!({ "path": "x" }));
    let mut live = Chat::default();
    live.apply(ThreadEvent::ToolStarted(read.clone()), 0);
    live.apply(ThreadEvent::ToolStarted(show.clone()), 0);
    live.apply(
        ThreadEvent::ToolFinished {
            call_id: "s".into(),
            result: ToolResult::text(shown.clone()),
        },
        0,
    );
    let restored = Chat::from_history(&[calling(vec![read, show]), result("s", &shown)]);
    for chat in [&live, &restored] {
        assert!(matches!(chat.items[0], Item::Tool(_)));
        assert!(matches!(chat.items[1], Item::Block(_)));
    }
}

/// A retry is what the indicator says, never a notice; the call's answer clears it.
#[test]
fn a_retry_is_the_indicator_not_a_notice() {
    let mut chat = Chat::default();
    chat.apply(
        ThreadEvent::Retrying {
            attempt: 1,
            error: "busy".into(),
            kind: crate::model::ProviderErrorKind::Overloaded,
            wait_ms: 1000,
            max_attempts: Some(3),
        },
        5,
    );
    assert!(chat.items.is_empty());
    let held = chat.work(5).expect("the retry shows");
    assert_eq!(held.kind, crate::thread::WorkKind::Held);
    assert!(held.label.ends_with("next in 1 s"), "{}", held.label);
    chat.apply(text("back"), 6);
    assert_eq!(chat.work(6), None, "no turn runs and the call answered");
}

/// One whole conversation, read from the history the core stores and followed live as
/// the core sends it, draws the same: the same items in the same order with the same
/// content and states, the same turn boundaries and the same skill changes. Only when
/// things came and which stored message an item shows may differ. This is the guard of
/// the whole model: a rule that one way follows and the other does not shows here.
#[test]
fn a_conversation_reads_the_same_from_history_and_live() {
    let at = |mut message: Message, sequence: u64| {
        message.sequence = sequence;
        message.message_id = format!("m{sequence}");
        message.created_at = sequence as i64 * 1000;
        message
    };
    let question = crate::tools::Question {
        question: "Which?".into(),
        options: vec!["A".into(), "B".into()],
        multiple: false,
    };
    let read = call("a", "read_file", json!({ "path": "src/lib.rs" }));
    let run = call("b", "run_command", json!({ "command": "cargo test" }));
    let ask_agent = call("c", "ask_agent", json!({ "task": "look around" }));
    let ask = call("q", "ask_user", serde_json::to_value(&question).unwrap());
    let answer = question.answer(&[0], "");
    let report = Message {
        from_thread_id: Some("sub1".into()),
        ..message(MessageRole::User, "the report")
    };
    let card = suggestion();
    let suggest = call("s", SUGGEST_TOOL, json!({ "ids": ["official/motion-kit"] }));
    let mut step = calling(vec![read.clone(), run.clone(), ask_agent.clone()]);
    step.content = "Let me look.".into();
    step.reasoning = Some("think".into());
    let history = [
        at(message(MessageRole::User, "go"), 1),
        at(step, 2),
        at(result("a", "file text"), 3),
        at(result("b", "error: boom"), 4),
        at(result("c", "sent"), 5),
        at(report.clone(), 6),
        at(calling(vec![ask.clone()]), 7),
        at(result("q", "asked"), 8),
        at(message(MessageRole::User, &answer), 9),
        at(message(MessageRole::Assistant, "done"), 10),
        at(message(MessageRole::User, "again"), 11),
        at(calling(vec![suggest.clone()]), 12),
        at(result("s", &card.result()), 13),
        at(message(MessageRole::Assistant, "second"), 14),
    ];
    let change = |undone: bool| SkillChange {
        id: 1,
        skill: "notes".into(),
        call_id: "a".into(),
        action: SkillSaveAction::Updated,
        reason: "why".into(),
        changed_at_ms: 0,
        undone,
        can_undo: !undone,
    };
    let mut stored = Chat::from_history(&history);
    stored.skills_loaded(vec![change(true)]);

    let finished = |id: &str, content: &str, is_error: bool| ThreadEvent::ToolFinished {
        call_id: id.into(),
        result: ToolResult {
            is_error,
            ..ToolResult::text(content.into())
        },
    };
    let ended = || Turn {
        status: TurnStatus::Completed,
        ..turn()
    };
    let events = [
        ThreadEvent::UserMessage(history[0].clone()),
        ThreadEvent::TurnStarted(turn()),
        ThreadEvent::Model(ModelEvent::Reasoning("think".into())),
        text("Let me look."),
        ThreadEvent::ToolStarted(read),
        ThreadEvent::ToolStarted(run),
        ThreadEvent::ToolStarted(ask_agent),
        ThreadEvent::SubagentStarted {
            call_id: "c".into(),
            thread_id: "sub1".into(),
            agent: "general".into(),
            task: "look around".into(),
            effort: None,
            model: None,
        },
        finished("a", "file text", false),
        ThreadEvent::SkillChanged(change(false)),
        finished("b", "error: boom", true),
        finished("c", "sent", false),
        ThreadEvent::FromAgent(report),
        ThreadEvent::ToolStarted(ask),
        finished("q", "asked", false),
        ThreadEvent::UserMessage(history[8].clone()),
        text("done"),
        ThreadEvent::SkillChanged(change(true)),
        ThreadEvent::TurnFinished(ended()),
        ThreadEvent::Idle,
        ThreadEvent::UserMessage(history[10].clone()),
        ThreadEvent::TurnStarted(turn()),
        ThreadEvent::ToolStarted(suggest),
        finished("s", &card.result(), false),
        text("second"),
        ThreadEvent::TurnFinished(ended()),
        ThreadEvent::Idle,
    ];
    let mut live = Chat::default();
    for (at, event) in events.into_iter().enumerate() {
        live.apply(event, at as i64);
    }

    assert_eq!(live.items, stored.items);
    assert_eq!(live.activity_boundaries, stored.activity_boundaries);
    assert_eq!(live.skill_changes, stored.skill_changes);
    // What it holds, to be sure the comparison saw the rules at work.
    assert!(matches!(&stored.items[3], Item::Tool(tool) if tool.state == ToolState::Failed));
    assert!(matches!(&stored.items[5], Item::Agent { from, .. } if *from == agent_name("sub1")));
    assert!(matches!(&stored.items[7], Item::User { answered, .. } if answered.len() == 1));
    assert!(matches!(&stored.items[10], Item::Suggestion(shown) if *shown == card));
    assert_eq!(stored.activity_boundaries, [1, 9, 10, 12]);
}

fn suggestion() -> Suggestion {
    let listing = Listing {
        name: "motion-kit".into(),
        source: "official".into(),
        version: "1.0.0".into(),
        description: "Interface animation.".into(),
        kind: Kind::Pack,
        ..Listing::default()
    };
    Suggestion {
        label: "Animation skills".into(),
        listings: vec![Suggested::of(&listing)],
    }
}

/// A card takes the place of its step only for `suggest_from_workshop`, and only when
/// its result holds one: a refused call, or another tool answering with the same text,
/// stays a step.
#[test]
fn a_workshop_card_replaces_only_its_own_call() {
    let card = suggestion();
    let finished = |id: &str, content: &str, is_error: bool| ThreadEvent::ToolFinished {
        call_id: id.into(),
        result: ToolResult {
            is_error,
            ..ToolResult::text(content.into())
        },
    };
    let mut chat = Chat::default();
    chat.apply(
        ThreadEvent::ToolStarted(call("s", SUGGEST_TOOL, json!({}))),
        1,
    );
    chat.apply(finished("s", &card.result(), false), 2);
    assert!(matches!(&chat.items[0], Item::Suggestion(shown) if *shown == card));

    chat.apply(
        ThreadEvent::ToolStarted(call("x", "mcp_echo", json!({}))),
        3,
    );
    chat.apply(finished("x", &card.result(), false), 4);
    assert!(matches!(&chat.items[1], Item::Tool(tool) if tool.call_id == "x"));

    chat.apply(
        ThreadEvent::ToolStarted(call("e", SUGGEST_TOOL, json!({}))),
        5,
    );
    chat.apply(finished("e", "error: not in the Workshop: a/b", true), 6);
    assert!(matches!(&chat.items[2], Item::Tool(tool) if tool.state == ToolState::Failed));

    let history = [
        calling(vec![call("e", SUGGEST_TOOL, json!({}))]),
        result("e", "error: not in the Workshop: a/b"),
    ];
    let stored = Chat::from_history(&history);
    assert!(matches!(&stored.items[0], Item::Tool(tool) if tool.state == ToolState::Failed));
}

/// `suggest_next` is no step, live or read back: its text reaches the composer in
/// `NextSuggested`, and the transcript shows nothing for it.
#[test]
fn a_suggested_next_message_is_no_step() {
    let suggest = call("s", SUGGEST_NEXT_TOOL, json!({ "text": "commit it" }));
    let mut live = Chat::default();
    live.apply(ThreadEvent::ToolStarted(suggest.clone()), 0);
    live.apply(
        ThreadEvent::ToolFinished {
            call_id: "s".into(),
            result: ToolResult::text("Suggested.".into()),
        },
        0,
    );
    live.apply(
        ThreadEvent::NextSuggested {
            text: "commit it".into(),
        },
        0,
    );
    let read = call("r", "read_file", json!({ "path": "a" }));
    let restored = Chat::from_history(&[
        calling(vec![read, suggest]),
        result("r", "text"),
        result("s", "Suggested."),
    ]);
    assert!(live.items.is_empty(), "{:?}", live.items);
    assert_eq!(restored.items.len(), 1);
    assert!(matches!(&restored.items[0], Item::Tool(tool) if tool.name == "read_file"));
}
