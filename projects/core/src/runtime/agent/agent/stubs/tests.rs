//! Which results a move takes and what it frees, on contexts built in memory. Moves in
//! running turns are tested with the scripted model in `runtime/thread/runner/tests`.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::{
    providers::ToolCall,
    state::{Ended, Stub},
};

fn message(sequence: u64, role: MessageRole, content: &str, tokens: u64) -> Message {
    Message {
        message_id: format!("id-{sequence}"),
        thread_id: "thread".into(),
        sequence,
        role,
        content: content.into(),
        reasoning: None,
        tool_calls: Vec::new(),
        tool_call_id: None,
        artifacts: Vec::new(),
        from_thread_id: None,
        from_job_id: None,
        content_tokens: tokens,
        tool_calls_tokens: 0,
        reasoning_tokens: 0,
        created_at: 0,
    }
}

/// A step that calls `tool` under `id`.
fn step(sequence: u64, id: &str, tool: &str, arguments: Value) -> Message {
    let mut step = message(sequence, MessageRole::Assistant, "", 10);
    step.tool_calls = vec![ToolCall {
        id: id.into(),
        name: tool.into(),
        arguments,
    }];
    step
}

/// A result of `tokens` tokens for the call `id`.
fn result(sequence: u64, id: &str, tokens: u64) -> Message {
    let content = "line of output\n".repeat(tokens as usize / 4);
    let mut result = message(sequence, MessageRole::Tool, &content, tokens);
    result.tool_call_id = Some(id.into());
    result
}

fn conversation(messages: Vec<Message>) -> Conversation {
    Conversation {
        messages,
        compaction: None,
        answered_through: 0,
        last_prompt: None,
        senders: HashMap::new(),
        stubs: HashMap::new(),
    }
}

fn limits(keep: u32, min_tokens: u64) -> Limits {
    let mut limits = Limits::default();
    limits.stub_results = true;
    limits.stub_keep_steps = keep;
    limits.stub_min_tokens = min_tokens;
    limits
}

fn taken(conversation: &Conversation, limits: &Limits, after: u64) -> Vec<u64> {
    candidates(conversation, limits, after)
        .iter()
        .map(|candidate| candidate.result.sequence)
        .collect()
}

/// Steps of `(tool, tokens)`, one call each, after a user message: the step at 2n, its
/// result at 2n + 1.
fn steps(of: &[(&str, u64)]) -> Conversation {
    let mut messages = vec![message(1, MessageRole::User, "go", 1)];
    for (index, (tool, tokens)) in of.iter().enumerate() {
        let at = 2 * index as u64 + 2;
        let id = format!("call_{at}");
        let arguments = json!({ "command": "make", "path": "a.rs", "pattern": "x" });
        messages.push(step(at, &id, tool, arguments));
        messages.push(result(at + 1, &id, *tokens));
    }
    conversation(messages)
}

#[test]
fn a_candidate_is_on_the_list_large_enough_and_old() {
    let conversation = steps(&[
        ("run_command", 300),
        ("ask_user", 300),
        ("read_file", 100),
        ("search_files", 300),
        ("run_command", 300),
    ]);
    // Steps at 2, 4, 6, 8, 10: with 2 kept, those at 8 and 10 are new.
    assert_eq!(taken(&conversation, &limits(2, 200), 0), [3]);
    assert_eq!(taken(&conversation, &limits(2, 50), 0), [3, 7]);
    assert_eq!(taken(&conversation, &limits(3, 50), 0), [3]);
    assert!(taken(&conversation, &limits(5, 50), 0).is_empty());
}

#[test]
fn a_large_result_is_old_after_two_steps() {
    let keep = limits(8, 200);
    let conversation = steps(&[("run_command", 4_000), ("read_file", 3_999)]);
    assert!(taken(&conversation, &keep, 0).is_empty(), "2 steps are new");
    let conversation = steps(&[
        ("run_command", 4_000),
        ("read_file", 3_999),
        ("run_command", 300),
    ]);
    assert_eq!(taken(&conversation, &keep, 0), [3]);
}

#[test]
fn a_result_with_a_stub_or_before_the_frontier_is_left_alone() {
    let mut conversation = steps(&[
        ("run_command", 300),
        ("run_command", 300),
        ("run_command", 300),
        ("run_command", 300),
    ]);
    let keep = limits(2, 200);
    assert_eq!(taken(&conversation, &keep, 0), [3, 5]);
    assert_eq!(frontier(&conversation), 0);
    let stub = Stub {
        text: "[put away]".into(),
        saved_tokens: 290,
        made_at_sequence: 9,
    };
    conversation.stubs.insert("id-3".into(), stub);
    assert_eq!(frontier(&conversation), 3);
    assert_eq!(taken(&conversation, &keep, 0), [5]);
    assert!(taken(&conversation, &keep, 5).is_empty());
}

/// Providers that number calls per step reuse IDs: a result answers the call before it.
#[test]
fn a_reused_call_id_pairs_with_the_newest_call_before_the_result() {
    let mut messages = vec![message(1, MessageRole::User, "go", 1)];
    messages.push(step(2, "call_1", "ask_user", json!({})));
    messages.push(result(3, "call_1", 300));
    messages.push(step(
        4,
        "call_1",
        "run_command",
        json!({ "command": "make" }),
    ));
    messages.push(result(5, "call_1", 300));
    for at in [6, 8] {
        messages.push(step(at, &format!("call_{at}"), "goal", json!({})));
        messages.push(result(at + 1, &format!("call_{at}"), 10));
    }
    let conversation = conversation(messages);
    assert_eq!(taken(&conversation, &limits(2, 200), 0), [5]);
}

#[test]
fn a_plan_frees_what_its_stubs_save_and_counts_what_stays_after_the_first() {
    let mut conversation = steps(&[
        ("run_command", 5_000),
        ("run_command", 300),
        ("run_command", 300),
    ]);
    // A system message is sent first, wherever it stands.
    let prompt = message(8, MessageRole::System, "plan prompt", 1_000);
    conversation.messages.push(prompt);
    let keep = limits(2, 200);
    let candidates = candidates(&conversation, &keep, 0);
    let end = CallEnd {
        ended: Ended::Failed,
        exit_code: Some(2),
    };
    let ends = HashMap::from([("id-3".to_owned(), end)]);
    let planned = plan(&conversation, &candidates, &ends).unwrap();

    assert_eq!(planned.stubs.len(), 1);
    let text = &planned.stubs[0].text;
    assert!(
        text.starts_with("[result 3 put away: run_command \"make\" failed, exit 2"),
        "{text}"
    );
    assert_eq!(planned.saved, saved_tokens(5_000, text));
    // From the result on: itself, two steps of 10 and their results of 300.
    let after = 5_000 + 10 + 300 + 10 + 300;
    assert_eq!(planned.remaining, after - planned.saved);
    // Without a stored end, the outcome is left out.
    let planned = plan(&conversation, &candidates, &HashMap::new()).unwrap();
    assert!(planned.stubs[0].text.contains("\"make\", "));
}

#[test]
fn a_stub_is_never_as_long_as_its_result() {
    let conversation = steps(&[
        ("run_command", 30),
        ("run_command", 300),
        ("run_command", 300),
    ]);
    let keep = limits(2, 20);
    let candidates = candidates(&conversation, &keep, 0);
    assert_eq!(candidates.len(), 1);
    assert!(plan(&conversation, &candidates, &HashMap::new()).is_none());
}

#[test]
fn a_warm_move_must_free_six_tenths_of_what_stays() {
    let planned = |saved, remaining| Move {
        stubs: Vec::new(),
        saved,
        remaining,
    };
    assert!(warm_pays(&planned(6_000, 10_000)));
    assert!(!warm_pays(&planned(5_999, 10_000)));
    assert!(warm_pays(&planned(4_000, 0)));
    assert!(warm_pays(&planned(u64::MAX, u64::MAX)), "no overflow");
}
