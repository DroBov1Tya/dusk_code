//! Goals: the evaluator's verdicts after each turn, the stall rule, background work and
//! failed turns.

use super::*;
use crate::state::{GoalEvaluation, GoalVerdict};
use dusk_protocol::thread::transcript::{ChatLog, Item};

fn said(text: &'static str) -> Vec<Step> {
    vec![Step::Text(text), Step::Done]
}

/// A turn that calls a tool and then answers: two scripts.
fn works() -> [Vec<Step>; 2] {
    [vec![Step::Call("working"), Step::Done], said("worked")]
}

/// The goal's user messages, in order.
async fn user_lines(state: &State) -> Vec<String> {
    transcript(state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::User)
        .map(|(_, text)| text)
        .collect()
}

/// The verdicts until the goal is cleared.
async fn verdicts_until_cleared(output: &mut ThreadOutput) -> Vec<GoalEvaluation> {
    let mut verdicts = Vec::new();
    loop {
        match output.next_event().await.expect("the thread ended") {
            ThreadEvent::GoalEvaluated(evaluation) => verdicts.push(evaluation),
            ThreadEvent::Goal { goal: None, .. } if !verdicts.is_empty() => return verdicts,
            _ => {}
        }
    }
}

/// The verdicts of a run of the thread, until it ends.
fn verdicts(events: &[ThreadEvent]) -> Vec<GoalVerdict> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::GoalEvaluated(evaluation) => Some(evaluation.verdict),
            _ => None,
        })
        .collect()
}

/// `/goal` starts a turn at once; each turn is judged, a goal not met goes on with the
/// evaluator's reason, and one met is cleared as achieved.
#[tokio::test]
async fn a_goal_goes_on_until_the_evaluator_finds_it_met() {
    let state = thread_state("goal_met").await;
    let config = live_config("goal_met_config");
    let mut scripts = works().to_vec();
    scripts.push(said("title"));
    scripts.push(said("NOT_MET: run the tests and show their output"));
    scripts.extend(works());
    scripts.push(said("MET: the tests pass"));
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        verdicts_until_cleared(&mut output).await
    };

    let (_, verdicts) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    let kinds: Vec<_> = verdicts.iter().map(|v| v.verdict).collect();
    assert_eq!(kinds, [GoalVerdict::NotMet, GoalVerdict::Met]);
    assert_eq!((verdicts[0].turn, verdicts[1].turn), (1, 2));
    assert_eq!(verdicts[1].reason, "the tests pass");
    assert!(state.goal("thread").await.unwrap().is_none());
    assert_eq!(
        user_lines(&state).await,
        [
            "Goal: build it",
            "The goal is not met yet: run the tests and show their output\nKeep working toward it.",
        ]
    );
    let history = state.list_messages("thread").await.unwrap();
    let reopened: ChatLog = ChatLog::from_history(&history);
    let shown: Vec<_> = reopened
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Goal(evaluation) => Some(evaluation.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(shown, verdicts);
    let requests = provider.requests.lock().unwrap();
    // The agent hears the goal last at every step.
    assert!(matches!(
        requests[0].last(),
        Some(ChatMessage::System(text)) if text.ends_with("Goal: build it")
    ));
    // The evaluator reads the conversation as data, with no tool to call.
    let ChatMessage::User(asked) = &requests[3][1] else {
        panic!("the evaluator got no conversation: {:?}", requests[3]);
    };
    assert!(asked.starts_with("Goal: build it\n"), "{asked}");
    assert!(asked.contains("AGENT CALLED echo"), "{asked}");
    assert!(asked.contains("RESULT: working"), "{asked}");
    assert_eq!(
        provider.efforts.lock().unwrap().len(),
        4,
        "agent steps only"
    );
    // The verdicts are kept in the chat for a reopened transcript, and never sent.
    assert!(
        !format!("{requests:?}").contains("\\\"verdict\\\""),
        "a stored verdict reached the model"
    );
}

/// A goal found impossible is cleared as failed; an answer that is no verdict is asked
/// again, and read as not met when it stays one.
#[tokio::test]
async fn an_unreadable_verdict_goes_on_and_an_impossible_goal_ends() {
    let state = thread_state("goal_impossible").await;
    let config = live_config("goal_impossible_config");
    let mut scripts = works().to_vec();
    scripts.push(said("title"));
    scripts.push(said("I think it went well"));
    scripts.push(said("Sure! MET"));
    scripts.extend(works());
    scripts.push(said("IMPOSSIBLE: the API it needs does not exist"));
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        verdicts_until_cleared(&mut output).await
    };

    let (_, verdicts) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    let kinds: Vec<_> = verdicts.iter().map(|v| v.verdict).collect();
    assert_eq!(kinds, [GoalVerdict::NotMet, GoalVerdict::Impossible]);
    assert!(
        verdicts[0]
            .reason
            .starts_with("The evaluator could not judge")
    );
    assert_eq!(verdicts[1].reason, "the API it needs does not exist");
    assert!(state.goal("thread").await.unwrap().is_none());
}

/// An evaluator that can never judge does not keep the goal going: after three turns it
/// gave no verdict on, the goal pauses with why.
#[tokio::test]
async fn an_evaluator_that_never_judges_pauses_the_goal() {
    let state = thread_state("goal_unjudged").await;
    let config = live_config("goal_unjudged_config");
    let mut scripts = works().to_vec();
    scripts.push(said("title"));
    for turn in 0..3 {
        if turn > 0 {
            scripts.extend(works());
        }
        scripts.push(said("I cannot say"));
        scripts.push(said("Still unsure"));
    }
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        next_goal_note(&mut output).await
    };

    let (_, (note, goal)) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(
        note,
        "Goal paused: the evaluator could not judge the last 3 turns; check the model, \
         then send a message to go on"
    );
    assert_eq!(goal.unwrap().status, GoalStatus::Paused);
    assert_eq!(state.count_turns("thread").await.unwrap(), 3);
}

/// Waits for the stall note and returns the goal it carried.
async fn next_stall(output: &mut ThreadOutput) -> Goal {
    loop {
        if let Some(ThreadEvent::Goal {
            goal: Some(goal),
            note,
            ..
        }) = output.next_event().await
            && note.starts_with("Goal stalled")
        {
            return goal;
        }
    }
}

/// Turns that only answer the evaluator stop the goal with it still set; the user's next
/// message starts it again, and the count with it.
#[tokio::test]
async fn a_goal_stalls_after_turns_without_tools_until_the_user_speaks() {
    let state = thread_state("goal_stall").await;
    let config = live_config("goal_stall_config");
    let talk = said("still thinking");
    let not_met = said("NOT_MET: nothing was run");
    let mut scripts = vec![talk.clone(), said("title"), not_met.clone()];
    for _ in 0..5 {
        scripts.extend([talk.clone(), not_met.clone()]);
    }
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        let first = next_stall(&mut output).await;
        input.send("keep going").await.unwrap();
        let second = next_stall(&mut output).await;
        (first, second)
    };

    let (_, (first, second)) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(first.status, GoalStatus::Active);
    assert_eq!(first.turns, 3);
    assert_eq!(second.turns, 6);
    let goal = state.goal("thread").await.unwrap().unwrap();
    assert_eq!(goal.status, GoalStatus::Active);
    let continued = "The goal is not met yet: nothing was run\nKeep working toward it.";
    assert_eq!(
        user_lines(&state).await,
        [
            "Goal: build it",
            continued,
            continued,
            "keep going",
            continued,
            continued,
        ]
    );
}

/// A turn that ends while a job runs is not judged; the turn after the job's result is.
#[tokio::test]
async fn a_turn_is_not_judged_while_background_work_runs() {
    let state = thread_state("goal_background").await;
    let config = live_config("goal_background_config");
    state
        .create_job("thread", "build", "cargo build", "other", "lease")
        .await
        .unwrap();
    let provider = provider(vec![
        said("the build runs"),
        said("title"),
        said("the build passed"),
        said("MET: the build passed"),
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = {
        let state = Arc::clone(&state);
        let provider = Arc::clone(&provider);
        async move {
            input.send("/goal build it").await.unwrap();
            loop {
                match output.next_event().await.expect("the thread ended") {
                    ThreadEvent::GoalEvaluated(_) => panic!("judged while the job runs"),
                    ThreadEvent::Idle => break,
                    _ => {}
                }
            }
            assert_eq!(provider.requests.lock().unwrap().len(), 2);
            state
                .finish_job("build", "other", "completed", "ok")
                .await
                .unwrap();
            input.send("is it done?").await.unwrap();
            drop(input);
            drain(&mut output).await
        }
    };

    let (_, events) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(verdicts(&events), [GoalVerdict::Met]);
    assert!(state.goal("thread").await.unwrap().is_none());
}

/// No retries inside a model call, so a failure goes straight to the turn.
fn failing_config(name: &str) -> Arc<LiveConfig> {
    Arc::new(temp_live_config_with(
        name,
        "limits:\n  max_attempts: 1\n  wait_for_provider: false\n",
    ))
}

/// Waits for the goal to change and returns its note and the goal.
async fn next_goal_note(output: &mut ThreadOutput) -> (String, Option<Goal>) {
    loop {
        if let Some(ThreadEvent::Goal { goal, note, .. }) = output.next_event().await
            && !note.is_empty()
            && !note.starts_with("Goal set")
            && !note.starts_with("Goal: trying again")
        {
            return (note, goal);
        }
    }
}

/// A provider that is busy gets the turn again three times, then the goal pauses.
#[tokio::test]
async fn a_busy_provider_is_tried_three_times_then_the_goal_pauses() {
    let state = thread_state("goal_retry").await;
    let config = failing_config("goal_retry_config");
    let busy = vec![Step::Fail];
    let provider = provider(vec![
        busy.clone(),
        said("title"),
        busy.clone(),
        busy.clone(),
        busy,
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        next_goal_note(&mut output).await
    };

    let (_, (note, goal)) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert!(note.starts_with("Goal paused: "), "{note}");
    let goal = goal.unwrap();
    assert_eq!(goal.status, GoalStatus::Paused);
    assert_eq!(state.count_turns("thread").await.unwrap(), 4);
}

/// A key the provider does not accept fails every turn: the goal is cleared.
#[tokio::test]
async fn an_unrecoverable_error_clears_the_goal() {
    let state = thread_state("goal_auth").await;
    let config = failing_config("goal_auth_config");
    let provider = provider(vec![
        vec![Step::Error(ProviderErrorKind::Auth, None)],
        said("title"),
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("/goal build it").await.unwrap();
        next_goal_note(&mut output).await
    };

    let (_, (note, goal)) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert!(
        note.starts_with("Goal cleared after an unrecoverable error"),
        "{note}"
    );
    assert!(goal.is_none());
    assert!(state.goal("thread").await.unwrap().is_none());
}

/// The user's stop pauses the goal; their next message goes on with it.
#[tokio::test]
async fn a_stop_pauses_the_goal_and_a_message_goes_on_with_it() {
    let state = thread_state("goal_stop").await;
    let config = live_config("goal_stop_config");
    let provider = provider(vec![vec![Step::Hang], said("went on"), said("MET: done")]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = {
        let state = Arc::clone(&state);
        let provider = Arc::clone(&provider);
        async move {
            input.send("/goal build it").await.unwrap();
            loop {
                if let Some(ThreadEvent::TurnStarted(_)) = output.next_event().await {
                    break;
                }
            }
            // A stop before the turn asks the model would leave the hang to the next one.
            while provider.requests.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            input.cancel().await.unwrap();
            let (note, goal) = next_goal_note(&mut output).await;
            assert_eq!(note, "Goal paused: stopped by the user");
            assert_eq!(goal.unwrap().status, GoalStatus::Paused);
            assert_eq!(
                state.goal("thread").await.unwrap().unwrap().status,
                GoalStatus::Paused
            );
            input.send("go on").await.unwrap();
            verdicts_until_cleared(&mut output).await
        }
    };

    let (_, verdicts) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].verdict, GoalVerdict::Met);
}

/// Turns without a tool before `/goal` do not count toward its stall rule: a first goal
/// turn that only plans goes on.
#[tokio::test]
async fn turns_before_a_goal_do_not_stall_it() {
    let state = thread_state("goal_fresh_count").await;
    let config = live_config("goal_fresh_count_config");
    let mut scripts = vec![said("hello"), said("title"), said("hi"), said("hey")];
    scripts.push(said("first I plan"));
    scripts.push(said("NOT_MET: nothing was built yet"));
    scripts.extend(works());
    scripts.push(said("MET: it is built"));
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        for text in ["one", "two", "three"] {
            input.send(text).await.unwrap();
            next_turn_finished(&mut output).await;
        }
        input.send("/goal build it").await.unwrap();
        let mut verdicts = Vec::new();
        loop {
            match output.next_event().await.expect("the thread ended") {
                ThreadEvent::GoalEvaluated(evaluation) => verdicts.push(evaluation.verdict),
                ThreadEvent::Goal { note, .. } if note.starts_with("Goal stalled") => {
                    panic!("stalled after {verdicts:?}")
                }
                ThreadEvent::Goal { goal: None, .. } if !verdicts.is_empty() => {
                    return verdicts;
                }
                _ => {}
            }
        }
    };

    let (_, verdicts) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(verdicts, [GoalVerdict::NotMet, GoalVerdict::Met]);
}
