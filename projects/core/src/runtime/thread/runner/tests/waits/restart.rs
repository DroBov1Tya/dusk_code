//! A wait for the provider and a restart: the next runner sleeps out the rest of the
//! wait, and the waits in a row stay bounded across it.

use super::*;

/// A kill during the wait: the wait was stored, the next runner sleeps out the rest of it
/// before anything else, tells it as a wait, and the turn then goes on once.
#[tokio::test]
async fn a_restart_during_the_wait_sleeps_the_rest_then_goes_on() {
    let state = thread_state("wait_restart").await;
    let config = config("wait_restart_config", "");
    let first = super::provider(vec![rate_limited(300)]);
    let (runner, input, mut output) = super::runner(&state, &first, &config, model(None));
    input.send("go").await.unwrap();
    let running = tokio::spawn(runner.run());
    let (waiting, _, _, wake_at) = next_wait(&mut output).await;
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    drop(input);
    let stored = state.unfinished_turn("thread").await.unwrap().unwrap().turn;
    assert_eq!(
        stored.waiting, waiting.waiting,
        "the wait survives the kill"
    );

    let second = super::provider(vec![answered("done")]);
    let (runner, input, output) = super::runner(&state, &second, &config, model(None));
    drop(input);
    let running = runner.run();
    let (_, events) = tokio::join!(running, async move {
        let mut output = output;
        drain(&mut output).await
    });

    let told = events.iter().find(|event| {
        matches!(
            event,
            ThreadEvent::TurnWaiting(_) | ThreadEvent::TurnRunning(_) | ThreadEvent::TurnStarted(_)
        )
    });
    assert_eq!(told, Some(&ThreadEvent::TurnWaiting(stored)));
    let sent = second.sent_at.lock().unwrap().clone();
    assert!(sent[0] >= wake_at, "sent at {} before {wake_at}", sent[0]);
    assert_eq!(first.sent_at.lock().unwrap().len(), 1);
    assert_eq!(
        transcript(&state).await,
        [
            (MessageRole::User, "go".to_owned()),
            (MessageRole::Assistant, "done".to_owned())
        ]
    );
    let turns = state.count_turns("thread").await.unwrap();
    assert_eq!(turns, 1, "the same turn went on");
    assert_eq!(newest_status(&state).await, "completed");
    assert_eq!(columns(&state, &waiting.turn_id).await, (None, None, None));
}

/// A restart during a wait that began long ago asks nobody whether to go on: the turn
/// was at work, waiting, so it sleeps out the rest of its wait and goes on.
#[tokio::test]
async fn a_restart_during_a_long_wait_goes_on_unasked() {
    let state = thread_state("wait_long_restart").await;
    state.add_user_message("thread", "go", "go").await.unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let wake_at = dusk_protocol::system::now_ms() + 100;
    let kind = ProviderErrorKind::Quota;
    let id = &turn.turn_id;
    state.wait_for_provider(id, kind, wake_at).await.unwrap();
    let two_hours = 2 * 3600 * 1000;
    for older in [
        "UPDATE turns SET started_at = started_at - ?1, waiting_since = waiting_since - ?1",
        "UPDATE messages SET created_at = created_at - ?1",
    ] {
        sqlx::query(older)
            .bind(two_hours)
            .execute(&state.pool)
            .await
            .unwrap();
    }
    let config = config("wait_long_restart_config", "");
    let provider = super::provider(vec![answered("done")]);
    let (runner, input, mut output) = super::runner(&state, &provider, &config, model(None));
    drop(input);
    runner.run().await;
    let events = drain(&mut output).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::Question(_))),
        "{events:?}"
    );
    assert!(provider.sent_at.lock().unwrap()[0] >= wake_at);
    assert_eq!(newest_status(&state).await, "completed");
}

/// Waits in a row keep when the first began. Past the bound from then, the turn fails
/// with the very event it failed with before turns waited.
#[tokio::test]
async fn waits_in_a_row_are_bounded_then_the_turn_fails_as_before() {
    let state = thread_state("wait_in_a_row").await;
    let config = config("wait_in_a_row_config", "");
    let provider = super::provider(vec![rate_limited(50), rate_limited(50), answered("done")]);
    let (runner, input, output) = super::runner(&state, &provider, &config, model(None));
    let events = run_go(runner, input, output).await;
    let [
        TurnWait::Provider { since, wake_at, .. },
        TurnWait::Provider {
            since: again,
            wake_at: later,
            ..
        },
    ] = waits(&events)[..]
    else {
        panic!("two waits: {events:?}");
    };
    assert_eq!(since, again);
    assert!(later > wake_at);
    assert_eq!(newest_status(&state).await, "completed");

    // As a restart finds a turn that has waited a day (86.4 s at the tests' scale).
    let failed = failing(ProviderErrorKind::RateLimited, None);
    let state = thread_state("wait_spent").await;
    state.add_user_message("thread", "go", "go").await.unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let now = dusk_protocol::system::now_ms();
    let kind = ProviderErrorKind::RateLimited;
    state
        .wait_for_provider(&turn.turn_id, kind, now)
        .await
        .unwrap();
    sqlx::query("UPDATE turns SET waiting_since = ? WHERE turn_id = ?")
        .bind(now - 90_000)
        .bind(&turn.turn_id)
        .execute(&state.pool)
        .await
        .unwrap();
    let provider = super::provider(vec![failed.clone()]);
    let (runner, input, mut output) = super::runner(&state, &provider, &config, model(None));
    drop(input);
    runner.run().await;
    let spent = drain(&mut output).await;

    // The same call with the turn not waiting at all.
    let before = thread_state("wait_off_failure").await;
    let off = self::config("wait_off_failure_config", "  wait_for_provider: false\n");
    let today = super::provider(vec![failed]);
    let (runner, input, output) = super::runner(&before, &today, &off, model(None));
    let today = run_go(runner, input, output).await;

    let expected = TurnFailure {
        error: "RateLimited".into(),
        provider: Some(ProviderFailure {
            kind: ProviderErrorKind::RateLimited,
            attempts: 1,
        }),
    };
    assert_eq!(failures(&today), [expected]);
    assert_eq!(failures(&spent), failures(&today));
    assert_eq!(provider.sent_at.lock().unwrap().len(), 1);
    assert_eq!(columns(&state, &turn.turn_id).await, (None, None, None));
    assert_eq!(newest_status(&state).await, "failed");
}
