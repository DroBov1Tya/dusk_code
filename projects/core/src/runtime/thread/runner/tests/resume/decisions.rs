//! The user's answers to what a restart asks, and what else may come meanwhile.

use super::*;

#[tokio::test]
async fn check_first_settles_the_calls_and_tells_the_model_once() {
    let state = thread_state("resume_check_first").await;
    killed_in_step(&state, &["a", "b"], 0).await;
    let counter = Counter::default();

    let (provider, runner, input, output) = restarted(&state, &counter, answered("checked"));
    let events = answer(runner, input, output, "Check first").await;

    assert!(counter.runs().is_empty());
    assert_eq!(results(&state, "a").await, [LOST]);
    assert_eq!(results(&state, "b").await, [NOT_RUN]);
    let history = transcript(&state).await;
    let notes = history
        .iter()
        .filter(|(role, text)| *role == MessageRole::System && text == CHECK_NOTE)
        .count();
    assert_eq!(notes, 1);
    // The note comes after the results, where the model reads it before going on.
    let note_at = history.iter().position(|(_, text)| text == CHECK_NOTE);
    let result_at = history.iter().position(|(_, text)| text == NOT_RUN);
    assert!(note_at > result_at, "{history:?}");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ThreadEvent::TurnRunning(_)))
    );
    assert_eq!(steps(&provider), 1);
    let request = provider.requests.lock().unwrap()[0].clone();
    assert!(request.contains(&ChatMessage::System(CHECK_NOTE.into())));
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

#[tokio::test]
async fn stop_interrupts_the_turn_as_a_restart_did() {
    let state = thread_state("resume_stop").await;
    killed_in_step(&state, &["a", "b"], 0).await;
    let counter = Counter::default();

    let (provider, runner, input, output) = restarted(&state, &counter, Vec::new());
    let events = answer(runner, input, output, "Stop").await;

    assert!(counter.runs().is_empty());
    assert_eq!(steps(&provider), 0);
    assert_eq!(results(&state, "a").await, [LOST]);
    assert_eq!(results(&state, "b").await, [LOST]);
    assert_eq!(newest_turn(&state).await, TurnStatus::Interrupted);
    assert!(events.iter().any(|e| matches!(
        e,
        ThreadEvent::TurnFinished(turn) if turn.status == TurnStatus::Interrupted
    )));
    let history = state.list_messages("thread").await.unwrap();
    assert!(history.iter().all(|m| !m.content.contains("QUESTION:")));
    assert!(state.pending_messages("thread").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_users_stop_while_asked_interrupts_the_turn() {
    let state = thread_state("resume_cancel").await;
    killed_in_step(&state, &["a"], 0).await;
    let counter = Counter::default();
    let (provider, runner, input, mut output) = restarted(&state, &counter, Vec::new());
    let running = tokio::spawn(runner.run());
    while !matches!(output.next_event().await, Some(ThreadEvent::Question(_))) {}
    input.cancel().await.unwrap();
    drop(input);
    drain(&mut output).await;
    running.await.unwrap();

    assert!(counter.runs().is_empty());
    assert_eq!(steps(&provider), 0);
    assert_eq!(results(&state, "a").await, [LOST]);
    assert_eq!(newest_turn(&state).await, TurnStatus::Interrupted);
}

#[tokio::test]
async fn another_message_while_asked_stops_the_turn_and_is_answered() {
    let state = thread_state("resume_other_message").await;
    let cut = killed_in_step(&state, &["a"], 0).await;
    let counter = Counter::default();
    let (provider, runner, input, mut output) = restarted(&state, &counter, answered("new answer"));
    let running = tokio::spawn(runner.run());
    while !matches!(output.next_event().await, Some(ThreadEvent::Question(_))) {}
    input.send("something else").await.unwrap();
    drop(input);
    drain(&mut output).await;
    running.await.unwrap();

    assert!(counter.runs().is_empty());
    assert_eq!(results(&state, "a").await, [LOST]);
    let status: String = sqlx::query_scalar("SELECT status FROM turns WHERE turn_id = ?")
        .bind(&cut.turn_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(status, "interrupted");
    let history = transcript(&state).await;
    let tail = &history[history.len() - 2..];
    assert_eq!(
        tail,
        [
            (MessageRole::User, "something else".to_owned()),
            (MessageRole::Assistant, "new answer".to_owned())
        ]
    );
    assert_eq!(steps(&provider), 1);
}

#[tokio::test]
async fn a_second_restart_while_asked_asks_again_and_writes_nothing() {
    let state = thread_state("resume_twice").await;
    killed_in_step(&state, &["a"], 0).await;
    let counter = Counter::default();
    let (_, runner, input, output) = restarted(&state, &counter, Vec::new());
    let first = question(&ask_only(runner, input, output).await);
    let messages = state.list_messages("thread").await.unwrap();
    let turn = state.unfinished_turn("thread").await.unwrap();

    let (provider, runner, input, output) = restarted(&state, &counter, Vec::new());
    let events = ask_only(runner, input, output).await;

    assert_eq!(question(&events), first);
    assert_eq!(state.list_messages("thread").await.unwrap(), messages);
    assert_eq!(state.unfinished_turn("thread").await.unwrap(), turn);
    assert!(counter.runs().is_empty());
    assert_eq!(steps(&provider), 0);
}

#[tokio::test]
async fn a_turn_cut_off_long_ago_asks_before_it_goes_on() {
    let state = thread_state("resume_stale").await;
    killed_in_step(&state, &["a"], 1).await;
    let two_hours = 2 * 3600 * 1000;
    for older in [
        "UPDATE turns SET started_at = started_at - ?",
        "UPDATE messages SET created_at = created_at - ?",
    ] {
        sqlx::query(older)
            .bind(two_hours)
            .execute(&state.pool)
            .await
            .unwrap();
    }
    let counter = Counter::default();

    let (provider, runner, input, output) = restarted(&state, &counter, Vec::new());
    let asked = question(&ask_only(runner, input, output).await);
    assert!(
        asked
            .question
            .starts_with("This chat was cut off 2h 0m ago while working."),
        "{}",
        asked.question
    );
    assert_eq!(asked.options, ["Continue", "Stop"]);
    assert_eq!(steps(&provider), 0);

    let (provider, runner, input, output) = restarted(&state, &counter, answered("done"));
    answer(runner, input, output, "Continue").await;
    assert_eq!(steps(&provider), 1);
    assert!(counter.runs().is_empty());
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

/// What the user sent while the turn ran is not an answer: it waits for its own turn.
#[tokio::test]
async fn a_message_sent_before_the_restart_waits_for_the_decision() {
    let state = thread_state("resume_earlier_message").await;
    killed_in_step(&state, &["a"], 0).await;
    state.add_pending("thread", "and then this").await.unwrap();
    let counter = Counter::default();
    let scripts = vec![
        vec![Step::Text("went on"), Step::Done],
        vec![Step::Text("and this"), Step::Done],
    ];

    let (provider, runner, input, output) = restarted(&state, &counter, scripts);
    answer(runner, input, output, "Continue").await;

    assert!(counter.runs().is_empty());
    assert_eq!(results(&state, "a").await, [LOST]);
    let history = transcript(&state).await;
    let tail = &history[history.len() - 3..];
    assert_eq!(
        tail,
        [
            (MessageRole::Assistant, "went on".to_owned()),
            (MessageRole::User, "and then this".to_owned()),
            (MessageRole::Assistant, "and this".to_owned()),
        ]
    );
    assert_eq!(steps(&provider), 2);
}
