//! A turn whose provider cannot serve it waits and goes on by itself: the wait is stored,
//! outlasts a restart, ends at the user's stop, ends early at the user's message, and is
//! bounded. Tests count a millisecond for a second (`UNIT`), so a minute's wait is 60 ms.

use super::*;
use dusk_protocol::model::{ProviderFailure, ProviderWaitKind};
use dusk_protocol::thread::{TurnFailure, TurnWait};

mod restart;

/// No retries inside a call, so a failure goes straight to the turn.
fn config(name: &str, extra: &str) -> Arc<LiveConfig> {
    let text = format!("limits:\n  max_attempts: 1\n{extra}");
    Arc::new(temp_live_config_with(name, &text))
}

fn failing(kind: ProviderErrorKind, retry_after_ms: Option<u64>) -> Vec<Step> {
    vec![Step::Error(kind, retry_after_ms.map(Duration::from_millis))]
}

fn rate_limited(retry_after_ms: u64) -> Vec<Step> {
    failing(ProviderErrorKind::RateLimited, Some(retry_after_ms))
}

fn answered(text: &'static str) -> Vec<Step> {
    vec![Step::Text(text), Step::Done]
}

/// The wait of the turn's `TurnWaiting`, or a panic on anything that ends the turn first.
async fn next_wait(output: &mut ThreadOutput) -> (Turn, ProviderWaitKind, i64, i64) {
    loop {
        match output.next_event().await.expect("the thread ended") {
            ThreadEvent::TurnWaiting(turn) => {
                let Some(TurnWait::Provider {
                    kind,
                    since,
                    wake_at,
                }) = turn.waiting
                else {
                    panic!("no provider wait: {turn:?}");
                };
                return (turn, kind, since, wake_at);
            }
            event @ (ThreadEvent::TurnFailed(_) | ThreadEvent::TurnFinished(_)) => {
                panic!("the turn ended without a wait: {event:?}")
            }
            _ => {}
        }
    }
}

/// Runs the thread on "go" until its client is done with it; every event it told.
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

fn failures(events: &[ThreadEvent]) -> Vec<TurnFailure> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnFailed(failure) => Some(failure.clone()),
            _ => None,
        })
        .collect()
}

fn waits(events: &[ThreadEvent]) -> Vec<TurnWait> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnWaiting(turn) => turn.waiting,
            _ => None,
        })
        .collect()
}

/// The three wait columns as stored.
async fn columns(state: &State, turn_id: &str) -> (Option<String>, Option<i64>, Option<i64>) {
    sqlx::query_as("SELECT waiting_for, waiting_since, wake_at FROM turns WHERE turn_id = ?")
        .bind(turn_id)
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

async fn newest_status(state: &State) -> String {
    sqlx::query_scalar("SELECT status FROM turns ORDER BY started_at DESC, rowid DESC")
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

/// A rate-limited call: the turn stays running with the wait stored and told, nothing
/// fails, and once the model answers the wait is over before any tool runs.
#[tokio::test]
async fn a_rate_limited_call_waits_then_goes_on_and_the_wait_ends() {
    let state = thread_state("wait_rate_limited").await;
    let config = config("wait_rate_limited_config", "");
    let call = Step::Echo(0, "a".into(), json!({ "text": "a" }));
    let provider = super::provider(vec![rate_limited(100), vec![call, Step::Done]]);
    let counter = Counter {
        hang: true,
        ..Counter::default()
    };
    let tool = Box::new(counter.clone());
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("go").await.unwrap();
    let running = tokio::spawn(runner.run());

    let (waiting, kind, _, _) = next_wait(&mut output).await;
    assert_eq!(kind, ProviderWaitKind::RateLimited);
    let stored = state.unfinished_turn("thread").await.unwrap().unwrap().turn;
    assert_eq!(stored.status, TurnStatus::Running);
    assert_eq!(stored.waiting, waiting.waiting);

    let mut events = Vec::new();
    loop {
        let event = output.next_event().await.expect("the thread ended");
        let started = matches!(event, ThreadEvent::ToolStarted(_));
        events.push(event);
        if started {
            break;
        }
    }
    assert!(events.iter().any(|event| matches!(
        event,
        ThreadEvent::TurnRunning(turn) if turn.waiting.is_none()
    )));
    assert!(failures(&events).is_empty());
    assert_eq!(columns(&state, &waiting.turn_id).await, (None, None, None));
    assert_eq!(newest_status(&state).await, "running");

    input.cancel().await.unwrap();
    drop(input);
    drain(&mut output).await;
    running.await.unwrap();
    assert_eq!(counter.runs(), ["a"]);
}

/// The next request goes no earlier than the wait's end, and the turn then completes.
#[tokio::test]
async fn no_request_is_sent_before_the_wait_ends() {
    let state = thread_state("wait_no_early_request").await;
    let config = config("wait_no_early_request_config", "");
    let provider = super::provider(vec![rate_limited(150), answered("done")]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;

    let [TurnWait::Provider { wake_at, .. }] = waits(&events)[..] else {
        panic!("one wait: {events:?}");
    };
    let sent = provider.sent_at.lock().unwrap().clone();
    assert!(sent[1] >= wake_at, "sent at {} before {wake_at}", sent[1]);
    assert!(wake_at >= sent[0] + 150);
    assert!(failures(&events).is_empty());
    let history = transcript(&state).await;
    assert_eq!(
        history.last(),
        Some(&(MessageRole::Assistant, "done".to_owned()))
    );
    assert_eq!(newest_status(&state).await, "completed");
}

/// A pause the provider asks for that is longer than the retry loop sleeps is not slept
/// inside it: the call ends at once and the turn stores the wait.
#[tokio::test]
async fn a_long_retry_after_is_stored_not_slept_inside_the_retry_loop() {
    let state = thread_state("wait_long_retry_after").await;
    let config = Arc::new(temp_live_config("wait_long_retry_after_config"));
    let provider = super::provider(vec![rate_limited(200), answered("done")]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::Retrying { .. })),
        "{events:?}"
    );
    assert_eq!(waits(&events).len(), 1);
    assert_eq!(newest_status(&state).await, "completed");
}

/// The user's stop during the wait ends the turn as interrupted, with no wait left.
#[tokio::test]
async fn a_stop_during_the_wait_ends_the_turn_interrupted() {
    let state = thread_state("wait_stop").await;
    let config = config("wait_stop_config", "");
    let provider = super::provider(vec![rate_limited(5_000)]);
    let (runner, input, mut output) = super::runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("go").await.unwrap();
        let (waiting, ..) = next_wait(&mut output).await;
        input.cancel().await.unwrap();
        let ended = next_turn_finished(&mut output).await;
        drop(input);
        (waiting, ended, drain(&mut output).await)
    };
    let watched = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(runner.run(), client).1
    });
    let (waiting, ended, rest) = watched.await.expect("the stop ends the wait at once");

    assert_eq!(ended.turn_id, waiting.turn_id);
    assert_eq!(ended.status, TurnStatus::Interrupted);
    assert_eq!(ended.waiting, None);
    assert!(failures(&rest).is_empty());
    assert_eq!(columns(&state, &waiting.turn_id).await, (None, None, None));
    assert_eq!(provider.sent_at.lock().unwrap().len(), 1);
}

/// The user's message during the wait wakes it: the step is tried again at once, and the
/// message is answered after, by its own turn.
#[tokio::test]
async fn a_user_message_during_the_wait_wakes_it_early() {
    let state = thread_state("wait_woken").await;
    let config = config("wait_woken_config", "");
    let provider = super::provider(vec![
        rate_limited(10_000),
        answered("done"),
        answered("Title"),
        answered("again"),
    ]);
    let (runner, input, mut output) = super::runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("go").await.unwrap();
        let (_, _, _, wake_at) = next_wait(&mut output).await;
        input.send("the key is fixed").await.unwrap();
        let first = next_turn_finished(&mut output).await;
        drop(input);
        drain(&mut output).await;
        (wake_at, first)
    };
    let watched = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(), client).1
    });
    let (wake_at, first) = watched.await.expect("the message ends the wait early");

    assert_eq!(first.status, TurnStatus::Completed);
    let sent = provider.sent_at.lock().unwrap().clone();
    assert!(
        sent[1] < wake_at,
        "the step went again before the wait's end"
    );
    let history = transcript(&state).await;
    assert_eq!(
        &history[history.len() - 2..],
        [
            (MessageRole::User, "the key is fixed".to_owned()),
            (MessageRole::Assistant, "again".to_owned())
        ]
    );
}

/// A spent quota is waited out like a rate limit; a key that is not accepted and a
/// request that is refused fail the turn at once, as before.
#[tokio::test]
async fn quota_waits_and_auth_or_refused_do_not() {
    let state = thread_state("wait_quota").await;
    let config = config("wait_quota_config", "");
    let quota = failing(ProviderErrorKind::Quota, Some(50));
    let provider = super::provider(vec![quota, answered("done")]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;
    let [TurnWait::Provider { kind, .. }] = waits(&events)[..] else {
        panic!("one wait: {events:?}");
    };
    assert_eq!(kind, ProviderWaitKind::Quota);
    assert_eq!(newest_status(&state).await, "completed");

    for kind in [ProviderErrorKind::Auth, ProviderErrorKind::Refused] {
        let state = thread_state(&format!("wait_{kind:?}")).await;
        let provider = super::provider(vec![failing(kind, None)]);
        let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
        let events = run_go(runner, input, output).await;
        assert!(waits(&events).is_empty(), "{kind:?}");
        let failed = failures(&events);
        assert_eq!(failed.len(), 1, "{kind:?}");
        assert_eq!(failed[0].provider.map(|failure| failure.kind), Some(kind));
        assert_eq!(newest_status(&state).await, "failed");
    }
}

/// With waits switched off, a provider that cannot serve the turn fails it as before:
/// the loop sleeps the pause it is asked for, and a spent quota fails at once.
#[tokio::test]
async fn with_waits_off_the_turn_fails_as_before() {
    let state = thread_state("wait_off").await;
    let off = "limits:\n  max_attempts: 2\n  wait_for_provider: false\n";
    let config = Arc::new(temp_live_config_with("wait_off_config", off));
    let provider = super::provider(vec![rate_limited(50), rate_limited(50)]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;

    assert!(waits(&events).is_empty());
    let retried: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::Retrying { wait_ms, .. } => Some(*wait_ms),
            _ => None,
        })
        .collect();
    assert_eq!(retried, [50], "the long pause is slept inside the loop");
    let failed = failures(&events);
    assert_eq!(
        failed[0].provider,
        Some(ProviderFailure {
            kind: ProviderErrorKind::RateLimited,
            attempts: 2,
        })
    );

    let state = thread_state("wait_off_quota").await;
    let provider = super::provider(vec![failing(ProviderErrorKind::Quota, None)]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;
    assert!(waits(&events).is_empty());
    assert_eq!(failures(&events).len(), 1);
    let steps = provider.efforts.lock().unwrap().len();
    assert_eq!(steps, 1, "the title aside, one call");
}

/// A goal's turn that waits for its provider is still the same turn: it is judged once,
/// after the wait, as it would have been without it.
#[tokio::test]
async fn a_goal_does_not_count_a_waiting_turn_as_a_round() {
    let state = thread_state("wait_goal").await;
    let config = config("wait_goal_config", "");
    // Each turn calls a tool, so the stall rule never stops the goal.
    let call = vec![Step::Call("working"), Step::Done];
    let worked = answered("worked");
    let not_met = answered("NOT_MET: keep going");
    let provider = super::provider(vec![
        rate_limited(50),
        call.clone(),
        worked.clone(),
        answered("title"),
        not_met.clone(),
        call.clone(),
        worked.clone(),
        not_met.clone(),
        call,
        worked,
        not_met,
        // The fourth turn fails, which pauses the goal.
        vec![Step::Reject],
    ]);
    let (runner, input, mut output) = super::runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        let mut waited = false;
        loop {
            match output.next_event().await.expect("the thread ended") {
                ThreadEvent::TurnWaiting(_) => waited = true,
                ThreadEvent::Goal {
                    goal: Some(goal), ..
                } if goal.status == GoalStatus::Paused => return (goal, waited),
                _ => {}
            }
        }
    };

    let (_, (goal, waited)) = tokio::join!(runner.run(), client);
    assert!(waited);
    assert_eq!(goal.turns, 3);
    let users: Vec<_> = transcript(&state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::User)
        .map(|(_, text)| text)
        .collect();
    assert_eq!(
        users,
        [
            "Goal: build it",
            "The goal is not met yet: keep going\nKeep working toward it.",
            "The goal is not met yet: keep going\nKeep working toward it.",
            "The goal is not met yet: keep going\nKeep working toward it.",
        ]
    );
}
