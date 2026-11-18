//! A thread's context with old tool results sent as their stubs. Providers cache a
//! request by its prefix, so these tests hold that a stubbed context is the same bytes
//! on every load and after a restart, that the switch is read when the context is, and
//! that the size of the context follows the stubs.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::Conversation;
use crate::{
    providers::{ChatMessage, ToolCall, Usage},
    state::{
        CallPurpose, Message, MessageRole, ModelCall, NewStub, State, TurnAnswer, TurnOutcome,
    },
    testing::temp_dir,
};

/// A thread with a prompt and three steps whose results are `results`, one per turn.
async fn thread(path: &Path, results: &[&str]) -> State {
    let state = State::open(path).await.unwrap();
    state.create_thread("thread", "test").await.unwrap();
    state.add_system_message("thread", "prompt").await.unwrap();
    for (index, result) in results.iter().enumerate() {
        let text = format!("question {index}");
        state
            .add_user_message("thread", &text, &text)
            .await
            .unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let call = ToolCall {
            id: format!("call-{index}"),
            name: "run_command".into(),
            arguments: json!({ "command": format!("cargo test {index}") }),
        };
        let called = TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        let id = &turn.turn_id;
        state
            .record_tool_calls(id, called, vec![call])
            .await
            .unwrap();
        state
            .record_tool_result(id, &format!("call-{index}"), (*result).into())
            .await
            .unwrap();
        let done = TurnAnswer {
            content: format!("answer {index}"),
            reasoning: None,
        };
        state
            .finish_turn(id, TurnOutcome::Completed(done))
            .await
            .unwrap();
    }
    state
}

fn db(name: &str) -> PathBuf {
    temp_dir(name).join("state.db")
}

async fn results(state: &State) -> Vec<Message> {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .into_iter()
        .filter(|message| message.role == MessageRole::Tool)
        .collect()
}

fn stub(message: &Message, text: &str) -> NewStub {
    NewStub {
        message_id: message.message_id.clone(),
        text: text.into(),
    }
}

async fn request(state: &State, stub_results: bool) -> String {
    let conversation = Conversation::load(state, "thread", stub_results)
        .await
        .unwrap();
    format!("{:?}", conversation.chat_messages())
}

async fn newest(state: &State) -> u64 {
    let messages = state.list_messages("thread").await.unwrap();
    messages.last().map_or(0, |message| message.sequence)
}

/// The stored sizes of the messages after `sequence`.
async fn sizes_after(state: &State, sequence: u64) -> u64 {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .iter()
        .filter(|message| message.sequence > sequence)
        .map(Message::context_tokens)
        .sum()
}

/// The provider counted a request through `through` as `prompt_tokens`.
async fn counted(state: &State, through: u64, prompt_tokens: u64) {
    let call = ModelCall {
        thread_id: "thread".into(),
        turn_id: None,
        purpose: CallPurpose::Answer,
        compaction_id: None,
        model: "model".into(),
        through_sequence: through,
    };
    let usage = Usage {
        prompt_tokens,
        ..Usage::default()
    };
    state.record_usage(&call, usage).await.unwrap();
}

#[tokio::test]
async fn a_stubbed_context_is_the_same_bytes_on_every_load_and_after_a_restart() {
    let path = db("stub_view_bytes");
    let long = "line\n".repeat(800);
    let state = thread(&path, &[&long, "short", &long]).await;
    let found = results(&state).await;
    let made_at = newest(&state).await;
    let stubs = [
        stub(&found[0], "[result 4 put away]"),
        stub(&found[2], "[result 12 put away]"),
    ];
    state.add_stubs("thread", made_at, &stubs).await.unwrap();

    let first = request(&state, true).await;
    let second = request(&state, true).await;
    state.close().await;
    let state = State::open(&path).await.unwrap();
    let reopened = request(&state, true).await;

    assert_eq!(first, second);
    assert_eq!(first, reopened);
    assert!(first.contains("[result 4 put away]") && first.contains("[result 12 put away]"));
    assert!(!first.contains("line\\n"), "no stubbed result is sent");
    assert!(
        first.contains("short"),
        "a result without a stub is sent whole"
    );
    state.close().await;
}

/// A stubbed result still answers its call: nothing but its text changes, so each call
/// keeps its result right after it.
#[tokio::test]
async fn a_stubbed_result_stays_paired_with_its_call() {
    let path = db("stub_view_pairs");
    let long = "x".repeat(4000);
    let state = thread(&path, &[&long, &long]).await;
    let found = results(&state).await;
    let whole = Conversation::load(&state, "thread", true).await.unwrap();
    let whole = whole.chat_messages();
    let made_at = newest(&state).await;
    let stubs = [stub(&found[0], "[put away]")];
    state.add_stubs("thread", made_at, &stubs).await.unwrap();

    let stubbed = Conversation::load(&state, "thread", true).await.unwrap();
    let stubbed = stubbed.chat_messages();

    assert_eq!(stubbed.len(), whole.len());
    let mut changed = 0;
    for (index, (before, after)) in whole.iter().zip(&stubbed).enumerate() {
        match (before, after) {
            (
                ChatMessage::Tool { call_id, .. },
                ChatMessage::Tool {
                    call_id: stubbed_id,
                    content,
                    ..
                },
            ) => {
                assert_eq!(call_id, stubbed_id);
                let Some(ChatMessage::Assistant { tool_calls, .. }) = stubbed.get(index - 1) else {
                    panic!("a result without its call before it");
                };
                assert_eq!(tool_calls[0].id, *stubbed_id);
                if before != after {
                    assert_eq!(content, "[put away]");
                    changed += 1;
                }
            }
            _ => assert_eq!(before, after),
        }
    }
    assert_eq!(changed, 1);
    state.close().await;
}

#[tokio::test]
async fn with_stubs_off_the_stored_text_is_sent_whatever_stubs_the_thread_holds() {
    let path = db("stub_view_off");
    let long = "y".repeat(4000);
    let state = thread(&path, &[&long, &long]).await;
    let found = results(&state).await;
    let before = request(&state, false).await;
    let tokens_before = Conversation::load(&state, "thread", false)
        .await
        .unwrap()
        .tokens(&[]);
    let made_at = newest(&state).await;
    let stubs = [stub(&found[0], "[put away]"), stub(&found[1], "[put away]")];
    state.add_stubs("thread", made_at, &stubs).await.unwrap();

    let after = request(&state, false).await;
    let conversation = Conversation::load(&state, "thread", false).await.unwrap();

    assert_eq!(before, after);
    assert!(!after.contains("[put away]"));
    assert!(conversation.stubs.is_empty());
    assert_eq!(conversation.tokens(&[]), tokens_before);
    assert_ne!(
        request(&state, true).await,
        after,
        "the stubs are there to use"
    );
    state.close().await;
}

/// The provider counted the request through the second step in full; stubs made since
/// shrink that count by what they save, and a later result is estimated by its stub.
#[tokio::test]
async fn the_size_of_the_context_drops_by_what_the_stubs_save() {
    let path = db("stub_view_tokens");
    let long = "z".repeat(4000);
    let state = thread(&path, &[&long, &long, &long]).await;
    let found = results(&state).await;
    // The request that asked for the third call: everything before its result.
    let through = found[2].sequence - 1;
    counted(&state, through, 10_000).await;
    let load = async || Conversation::load(&state, "thread", true).await.unwrap();
    let plain = load().await.tokens(&[]);
    assert_eq!(plain, 10_000 + sizes_after(&state, through).await);

    let made_at = newest(&state).await;
    let stubs = [
        stub(&found[0], "[first put away]"),
        stub(&found[2], "[third put away]"),
    ];
    state.add_stubs("thread", made_at, &stubs).await.unwrap();

    let conversation = load().await;
    let saved: u64 = conversation
        .stubs
        .values()
        .map(|stub| stub.saved_tokens)
        .sum();
    assert!(saved > 1_500, "{saved}");
    assert_eq!(conversation.tokens(&[]), plain - saved);
    // Once the provider has counted a request with the stubs, its count holds them.
    counted(&state, made_at, 7_000).await;
    assert_eq!(load().await.tokens(&[]), 7_000);
    state.close().await;
}

/// A context without stubs, or never counted by a provider, is sized as it always was.
#[tokio::test]
async fn without_stubs_the_size_of_the_context_is_as_before() {
    let path = db("stub_view_no_stubs");
    let state = thread(&path, &["one", "two"]).await;
    let conversation = Conversation::load(&state, "thread", true).await.unwrap();
    assert_eq!(conversation.tokens(&[]), sizes_after(&state, 0).await);
    counted(&state, 4, 900).await;
    let conversation = Conversation::load(&state, "thread", true).await.unwrap();
    assert_eq!(conversation.tokens(&[]), 900 + sizes_after(&state, 4).await);
    state.close().await;
}
