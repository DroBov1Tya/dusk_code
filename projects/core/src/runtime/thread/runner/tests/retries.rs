//! A model call that fails: one row while it is tried again, one event when it fails for
//! good, the same row after the chat is opened again, and the provider's health.

use super::*;
use dusk_protocol::model::{CallHealth, ProviderFailure};
use dusk_protocol::thread::{Note, TurnFailure, note};

/// Runs the thread until the client is done with it and returns every event it told.
async fn events_of(runner: ThreadRunner, mut output: ThreadOutput) -> Vec<ThreadEvent> {
    runner.run().await;
    let mut events = Vec::new();
    while let Some(event) = output.next_event().await {
        events.push(event);
    }
    events
}

fn failures(events: &[ThreadEvent]) -> Vec<&TurnFailure> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnFailed(failure) => Some(failure),
            _ => None,
        })
        .collect()
}

/// Every attempt failed: the turn ends with one event that says why and after how many
/// attempts, and nothing else in the chat says it again. A chat opened again shows it.
#[tokio::test]
async fn a_failed_call_ends_the_turn_with_one_event_that_a_reopened_chat_shows_again() {
    let state = thread_state("one_failure").await;
    let config = Arc::new(temp_live_config_with(
        "one_failure_config",
        "limits:\n  max_attempts: 3\n",
    ));
    let provider = provider(vec![vec![Step::Fail]; 3]);
    let (first, input, output) = runner(&state, &provider, &config, model(None));
    input.send("hello").await.unwrap();
    drop(input);
    let events = events_of(first, output).await;

    let expected = TurnFailure {
        error: "server busy".into(),
        provider: Some(ProviderFailure {
            kind: ProviderErrorKind::Overloaded,
            attempts: 3,
        }),
    };
    assert_eq!(failures(&events), [&expected]);
    let at = events
        .iter()
        .position(|event| matches!(event, ThreadEvent::TurnFailed(_)))
        .unwrap();
    assert!(
        matches!(&events[at + 1], ThreadEvent::TurnFinished(turn) if turn.status == TurnStatus::Failed)
    );
    let limits: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::Retrying { max_attempts, .. } => Some(*max_attempts),
            _ => None,
        })
        .collect();
    assert_eq!(limits, [Some(3), Some(3)]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::Failed(_) | ThreadEvent::NoModel))
    );
    let errors = events
        .iter()
        .filter(|event| matches!(note(event), Some(Note::Error(_))))
        .count();
    assert_eq!(errors, 1, "one row says the turn failed");

    // Opened again, the chat hears of it before anything else happens.
    assert_eq!(
        state.last_failure("thread").await.unwrap(),
        Some(expected.clone())
    );
    let (again, input, output) = runner(&state, &provider, &config, model(None));
    drop(input);
    let reopened = events_of(again, output).await;
    assert_eq!(failures(&reopened), [&expected]);
}

/// A host that cannot be reached is tried past `max_attempts`, for the time budget, and
/// the client is told no attempt count, since there is none.
#[tokio::test]
async fn an_unreachable_host_is_tried_past_the_attempts_until_the_time_budget() {
    let state = thread_state("unreachable").await;
    let budget: u64 = 60;
    let config = Arc::new(temp_live_config_with(
        "unreachable_config",
        &format!("limits:\n  max_attempts: 2\n  retry_unreachable_seconds: {budget}\n"),
    ));
    let provider = provider(vec![vec![Step::Unreachable]; 40]);
    let (runner, input, output) = runner(&state, &provider, &config, model(None));
    input.send("hello").await.unwrap();
    drop(input);
    let events = events_of(runner, output).await;

    let mut waited = 0;
    let mut retries = 0;
    for event in &events {
        if let ThreadEvent::Retrying {
            wait_ms,
            max_attempts,
            kind,
            ..
        } = event
        {
            assert_eq!(
                (*max_attempts, *kind),
                (None, ProviderErrorKind::Unreachable)
            );
            waited += wait_ms;
            retries += 1;
        }
    }
    assert!(retries >= 2, "tried past max_attempts: {retries} retries");
    // Tests count a millisecond for a second; each pause was planned inside the budget.
    assert!(waited <= budget, "waited {waited} of {budget}");
    let failed = failures(&events);
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].provider,
        Some(ProviderFailure {
            kind: ProviderErrorKind::Unreachable,
            attempts: retries + 1,
        })
    );
    assert!(
        provider.scripts.lock().unwrap().len() > 30,
        "it stopped at the budget"
    );
}

/// The user's stop during a pause between attempts ends the turn as stopped, not failed.
#[tokio::test]
async fn a_stop_while_a_call_waits_to_be_tried_again_ends_the_turn_as_stopped() {
    let state = thread_state("stop_retry").await;
    let config = Arc::new(temp_live_config_with(
        "stop_retry_config",
        "limits:\n  retry_unreachable_seconds: 3600\n",
    ));
    let provider = provider(vec![vec![Step::Unreachable]; 400]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("hello").await.unwrap();
        let mut seen = Vec::new();
        loop {
            let event = output.next_event().await.expect("the thread ended");
            if matches!(event, ThreadEvent::Retrying { attempt: 1, .. }) {
                input.cancel().await.unwrap();
            }
            if let ThreadEvent::TurnFinished(turn) = &event {
                assert_eq!(turn.status, TurnStatus::Interrupted);
                break;
            }
            seen.push(event);
        }
        assert!(failures(&seen).is_empty());
    };
    tokio::join!(runner.run(), client);
}

/// A provider that fails is shown failing, then answering once a call is answered; the
/// calls in between change nothing a client is sent.
#[tokio::test]
async fn a_provider_is_shown_failing_then_answering() {
    let state = thread_state("health").await;
    let config = Arc::new(temp_live_config_with(
        "health_config",
        "limits:\n  max_attempts: 2\n",
    ));
    let provider = provider(vec![
        vec![Step::Fail],
        vec![Step::Fail],
        // The thread's title, asked for after its first turn.
        vec![Step::Text("Title"), Step::Done],
        vec![Step::Text("ok"), Step::Done],
        vec![Step::Text("again"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let health = Arc::clone(&runner.agent.health);
    let shown = move || {
        let all = health.of(|_| true);
        assert_eq!(all.len(), 1);
        all[0].calls
    };
    let client = async move {
        input.send("first").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Failed
        );
        let failing = shown();
        assert!(matches!(
            failing,
            CallHealth::Failing {
                kind: ProviderErrorKind::Overloaded,
                last_answered: None,
                ..
            }
        ));
        input.send("second").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
        let answering = shown();
        assert!(matches!(answering, CallHealth::Answering { .. }));
        input.send("third").await.unwrap();
        next_turn_finished(&mut output).await;
        assert_eq!(shown(), answering, "a further answer changes nothing shown");
    };
    tokio::join!(runner.run(), client);
}
