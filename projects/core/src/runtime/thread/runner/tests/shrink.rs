//! A context too large for the model is compacted and the step taken again, twice at most;
//! with `shrink_context`, a summary whose own request is too large covers the older half.

use super::*;
use crate::state::{TurnAnswer, TurnOutcome};
use dusk_protocol::model::ProviderFailure;
use dusk_protocol::thread::TurnFailure;

fn too_long() -> Vec<Step> {
    vec![Step::Error(ProviderErrorKind::TooLong, None)]
}

fn answered(text: &'static str) -> Vec<Step> {
    vec![Step::Text(text), Step::Done]
}

async fn run_go(
    runner: ThreadRunner,
    input: ThreadInput,
    mut output: ThreadOutput,
) -> Vec<ThreadEvent> {
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;
    drain(&mut output).await
}

fn count(events: &[ThreadEvent], what: fn(&ThreadEvent) -> bool) -> usize {
    events.iter().filter(|event| what(event)).count()
}

#[tokio::test]
async fn a_context_too_large_is_compacted_once_then_the_step_goes_on() {
    let state = thread_state("shrink_once").await;
    let config = live_config("shrink_once_config");
    let scripts = vec![too_long(), answered("summary"), answered("done")];
    let provider = provider(scripts);
    let (runner, input, output) = runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;

    let started = |e: &ThreadEvent| matches!(e, ThreadEvent::CompactionStarted);
    let finished = |e: &ThreadEvent| matches!(e, ThreadEvent::CompactionFinished(_));
    let failed = |e: &ThreadEvent| matches!(e, ThreadEvent::TurnFailed(_));
    assert_eq!(count(&events, started), 1);
    assert_eq!(count(&events, finished), 1);
    assert_eq!(count(&events, failed), 0);
    let history = transcript(&state).await;
    assert_eq!(
        history.last(),
        Some(&(MessageRole::Assistant, "done".to_owned()))
    );
    assert!(state.latest_compaction("thread").await.unwrap().is_some());
}

/// Two compactions and still too large: the turn fails as it did before it compacted.
#[tokio::test]
async fn a_context_still_too_large_after_two_compactions_fails_as_before() {
    let state = thread_state("shrink_twice").await;
    let config = live_config("shrink_twice_config");
    // The second compaction finds nothing new to summarize and asks no model.
    let scripts = vec![too_long(), answered("summary"), too_long(), too_long()];
    let provider = provider(scripts);
    let (runner, input, output) = runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;

    let started = |e: &ThreadEvent| matches!(e, ThreadEvent::CompactionStarted);
    assert_eq!(count(&events, started), 2);
    let failures: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnFailed(failure) => Some(failure.clone()),
            _ => None,
        })
        .collect();
    let expected = TurnFailure {
        error: "TooLong".into(),
        provider: Some(ProviderFailure {
            kind: ProviderErrorKind::TooLong,
            attempts: 1,
        }),
    };
    assert_eq!(failures, [expected]);
    assert!(provider.scripts.lock().unwrap().is_empty(), "three steps");
}

/// A thread whose older half ends on a tool call: the user's first message, a step whose
/// call has a long result, then a long answer to a second message.
async fn thread_with_a_step(name: &str) -> (Arc<State>, u64) {
    let state = thread_state(name).await;
    let first = state
        .add_user_message("thread", "u1", "first")
        .await
        .unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let call = ToolCall {
        id: "a".into(),
        name: "echo".into(),
        arguments: json!({ "text": "a" }),
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
        .record_tool_result(id, "a", "x".repeat(2000))
        .await
        .unwrap();
    let done = TurnAnswer {
        content: "done".into(),
        reasoning: None,
    };
    state
        .finish_turn(id, TurnOutcome::Completed(done))
        .await
        .unwrap();
    state
        .add_user_message("thread", "u2", "second")
        .await
        .unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let long = TurnAnswer {
        content: "y".repeat(2000),
        reasoning: None,
    };
    let ended = TurnOutcome::Completed(long);
    state.finish_turn(&turn.turn_id, ended).await.unwrap();
    (state, first.sequence)
}

/// `/compact` on a context whose summary request is itself too large: off, it fails as
/// before; with `shrink_context`, it summarizes the older half, which ends before the
/// step, so the call stays with its result.
#[tokio::test]
async fn with_shrink_context_a_summary_too_large_covers_the_older_half() {
    for shrink in [false, true] {
        let name = format!("shrink_half_{shrink}");
        let (state, first) = thread_with_a_step(&name).await;
        let text = format!("limits:\n  shrink_context: {shrink}\n");
        let config = Arc::new(temp_live_config_with(&format!("{name}_config"), &text));
        // A summary that fails is asked once more without tools, as any summary is.
        let scripts = vec![too_long(), too_long(), answered("older half")];
        let provider = provider(scripts);
        let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
        input.send("/compact").await.unwrap();
        drop(input);
        runner.run().await;
        let events = drain(&mut output).await;

        let compacted = state.latest_compaction("thread").await.unwrap();
        let requests = provider.requests.lock().unwrap().clone();
        if !shrink {
            assert!(compacted.is_none());
            assert_eq!(requests.len(), 2);
            let failed = |e: &ThreadEvent| matches!(e, ThreadEvent::CompactionFailed { .. });
            assert_eq!(count(&events, failed), 1);
            continue;
        }
        let compacted = compacted.expect("the older half is summarized");
        assert_eq!(compacted.through_sequence, first);
        assert_eq!(compacted.summary, "older half");
        let messages = state.list_messages("thread").await.unwrap();
        let after = messages
            .iter()
            .find(|m| m.sequence > first && m.role != MessageRole::System)
            .unwrap();
        assert_ne!(after.role, MessageRole::Tool, "a step is not cut in two");
        // The request for the half no longer carries the step's long result.
        assert_eq!(requests.len(), 3);
        let long = "x".repeat(2000);
        let carries = |request: &Vec<ChatMessage>| {
            request.iter().any(|message| match message {
                ChatMessage::Tool { content, .. } => *content == long,
                _ => false,
            })
        };
        assert!(carries(&requests[0]));
        assert!(!carries(&requests[2]));
    }
}
