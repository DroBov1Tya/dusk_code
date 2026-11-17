//! A kill at each place a turn can be, and what the restart makes of it.

use super::*;

#[tokio::test]
async fn a_kill_before_the_models_answer_gives_one_answer() {
    let state = thread_state("resume_before_answer").await;
    state.add_user_message("thread", "go", "go").await.unwrap();
    state.start_turn("thread").await.unwrap().unwrap();

    let (provider, runner, input, _output) =
        restarted(&state, &Counter::default(), answered("answer"));
    drop(input);
    runner.run().await;

    assert_eq!(
        transcript(&state).await,
        [
            (MessageRole::User, "go".to_owned()),
            (MessageRole::Assistant, "answer".to_owned())
        ]
    );
    assert_eq!(steps(&provider), 1);
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

#[tokio::test]
async fn a_kill_mid_stream_keeps_no_partial_text() {
    let state = thread_state("resume_mid_stream").await;
    let first = provider(vec![vec![Step::Text("partial"), Step::Hang]]);
    let config = live_config("resume_mid_stream_config");
    let (runner, input, mut output) = runner(&state, &first, &config, model(None));
    input.send("go").await.unwrap();
    let running = tokio::spawn(runner.run());
    while !matches!(output.next_event().await, Some(ThreadEvent::Model(_))) {}
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());

    let (provider, runner, input, _output) =
        restarted(&state, &Counter::default(), answered("answer"));
    drop(input);
    runner.run().await;

    assert_eq!(
        transcript(&state).await,
        [
            (MessageRole::User, "go".to_owned()),
            (MessageRole::Assistant, "answer".to_owned())
        ]
    );
    assert_eq!(steps(&provider), 1);
}

#[tokio::test]
async fn a_repeatable_call_cut_off_before_it_started_runs_once() {
    let state = thread_state("resume_repeatable").await;
    killed_in_step(&state, &["a"], 0).await;
    let counter = Counter {
        repeatable: true,
        ..Counter::default()
    };

    let (provider, runner, input, _output) = restarted(&state, &counter, answered("done"));
    drop(input);
    runner.run().await;

    assert_eq!(counter.runs(), ["a"]);
    assert_eq!(results(&state, "a").await, ["a"]);
    assert_eq!(steps(&provider), 1);
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

#[tokio::test]
async fn a_call_that_may_not_repeat_is_not_run_and_the_user_is_asked() {
    let state = thread_state("resume_not_repeatable").await;
    let turn = killed_in_step(&state, &["a"], 0).await;
    let counter = Counter::default();

    let (provider, runner, input, output) = restarted(&state, &counter, Vec::new());
    let events = ask_only(runner, input, output).await;

    assert!(counter.runs().is_empty());
    assert!(results(&state, "a").await.is_empty());
    assert_eq!(steps(&provider), 0);
    let waiting = state.unfinished_turn("thread").await.unwrap().unwrap().turn;
    assert_eq!(waiting.turn_id, turn.turn_id);
    let Some(TurnWait::Decision { .. }) = waiting.waiting else {
        panic!("no decision wait: {waiting:?}");
    };
    assert!(events.contains(&ThreadEvent::TurnWaiting(waiting)));
    let asked = question(&events);
    assert_eq!(
        asked.question,
        "A restart cut off `echo`: {\"text\":\"a\"}. It may or may not have taken effect."
    );
    assert_eq!(asked.options, ["Continue", "Check first", "Stop"]);
}

#[tokio::test]
async fn a_call_cut_off_after_its_effect_never_runs_again() {
    let state = thread_state("resume_after_effect").await;
    let counter = Counter {
        hang: true,
        ..Counter::default()
    };
    let first = provider(vec![vec![
        Step::Echo(0, "a".into(), json!({ "text": "a" })),
        Step::Done,
    ]]);
    let config = live_config("resume_after_effect_config");
    let tool = Box::new(counter.clone());
    let (runner, input, _output) = runner_with(&state, &first, &config, model(None), tool);
    input.send("go").await.unwrap();
    let running = tokio::spawn(runner.run());
    while counter.runs().is_empty() {
        tokio::task::yield_now().await;
    }
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());

    let counter = Counter {
        hang: false,
        ..counter
    };
    let (provider, runner, input, output) = restarted(&state, &counter, answered("done"));
    answer(runner, input, output, "Continue").await;

    assert_eq!(counter.runs(), ["a"], "the effect happened once");
    assert_eq!(results(&state, "a").await, [LOST]);
    assert_eq!(outcome(&state, "a").await, "unknown");
    assert_eq!(steps(&provider), 1);
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

#[tokio::test]
async fn a_call_with_its_result_is_not_run_again() {
    let state = thread_state("resume_second_call").await;
    killed_in_step(&state, &["a", "b"], 1).await;
    let counter = Counter {
        repeatable: true,
        ..Counter::default()
    };

    let (_, runner, input, _output) = restarted(&state, &counter, answered("done"));
    drop(input);
    runner.run().await;

    assert_eq!(counter.runs(), ["b"]);
    assert_eq!(results(&state, "a").await, ["a"]);
    assert_eq!(results(&state, "b").await, ["b"]);
}

#[tokio::test]
async fn a_kill_after_the_last_result_runs_no_tool_again() {
    let state = thread_state("resume_after_results").await;
    killed_in_step(&state, &["a"], 1).await;
    let counter = Counter::default();

    let (provider, runner, input, _output) = restarted(&state, &counter, answered("done"));
    drop(input);
    runner.run().await;

    assert!(counter.runs().is_empty());
    assert_eq!(results(&state, "a").await, ["a"]);
    assert_eq!(steps(&provider), 1);
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

/// The answer was stored, and the process stopped before the runner applied it.
#[tokio::test]
async fn a_stored_decision_is_applied_once_and_kept_out_of_the_history() {
    let state = thread_state("resume_stored_answer").await;
    killed_in_step(&state, &["a", "b"], 0).await;
    let counter = Counter::default();
    let (_, runner, input, output) = restarted(&state, &counter, Vec::new());
    let asked = question(&ask_only(runner, input, output).await);
    state
        .add_pending("thread", &asked.reply("Continue"))
        .await
        .unwrap();

    let (provider, runner, input, _output) = restarted(&state, &counter, answered("done"));
    drop(input);
    runner.run().await;

    assert!(counter.runs().is_empty());
    assert_eq!(results(&state, "a").await, [LOST]);
    assert_eq!(results(&state, "b").await, [NOT_RUN]);
    assert_eq!(outcome(&state, "b").await, "cancelled");
    assert!(state.pending_messages("thread").await.unwrap().is_empty());
    let history = state.list_messages("thread").await.unwrap();
    assert!(
        history.iter().all(|m| !m.content.contains("QUESTION:")),
        "{history:?}"
    );
    assert_eq!(steps(&provider), 1);
    assert_eq!(newest_turn(&state).await, TurnStatus::Completed);
}

/// Either the answer was stored with the turn's end, or neither was.
#[tokio::test]
async fn a_kill_around_the_turns_end_gives_one_answer() {
    let state = thread_state("resume_turn_end").await;
    killed_in_step(&state, &["a"], 1).await;
    let (provider, runner, input, _output) =
        restarted(&state, &Counter::default(), answered("answer"));
    drop(input);
    runner.run().await;
    assert_eq!(steps(&provider), 1);
    let answers = |messages: Vec<crate::state::Message>| {
        messages
            .into_iter()
            .filter(|m| m.role == MessageRole::Assistant && m.content == "answer")
            .count()
    };
    assert_eq!(answers(state.list_messages("thread").await.unwrap()), 1);

    // The end was stored: another restart finds nothing to do.
    let (provider, runner, input, _output) =
        restarted(&state, &Counter::default(), answered("again"));
    drop(input);
    runner.run().await;
    assert_eq!(steps(&provider), 0);
    assert_eq!(answers(state.list_messages("thread").await.unwrap()), 1);
    assert!(state.unfinished_turn("thread").await.unwrap().is_none());
}

/// A chat and its agent were both at work. The agent's call may not repeat, so it stops,
/// and its chat is told so once; the chat's turn goes on.
#[tokio::test]
async fn an_agent_stopped_by_a_restart_tells_its_chat_once() {
    use crate::{init::paths::Paths, runtime::Runtime};
    let root = temp_dir("resume_agent");
    let cwd = root.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let scripts = (0..6).map(|_| vec![Step::Text("ok"), Step::Done]).collect();
    let fake = provider(scripts);
    runtime.launcher.model.set(Choice {
        provider_name: "fake".into(),
        provider: Arc::clone(&fake) as Arc<dyn ModelProvider>,
        model: model(None),
    });
    let state = runtime.state();
    state.create_thread("chat", "test").await.unwrap();
    state.add_user_message("chat", "go", "go").await.unwrap();
    state.start_turn("chat").await.unwrap().unwrap();
    state
        .create_agent_thread("child", "chat", "agent", "child")
        .await
        .unwrap();
    // As `ask_agent` makes it.
    let workspace = runtime.launcher.paths.thread_workspace("child").unwrap();
    std::fs::create_dir_all(workspace).unwrap();
    state
        .add_user_message("child", "task", "task")
        .await
        .unwrap();
    let child = state.start_turn("child").await.unwrap().unwrap();
    let call = ToolCall {
        id: "run".into(),
        name: "run_command".into(),
        arguments: json!({ "command": "touch made" }),
    };
    let answer = TurnAnswer {
        content: String::new(),
        reasoning: None,
    };
    state
        .record_tool_calls(&child.turn_id, answer, vec![call])
        .await
        .unwrap();

    let mut handle = runtime.resume_thread("chat", &cwd).await.unwrap();
    let mut finished = 0;
    let wait = async {
        while finished < 2 {
            if let Some(ThreadEvent::TurnFinished(_)) = handle.output.next_event().await {
                finished += 1;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .expect("the chat's resumed turn and the turn after the report end");

    let messages = state.list_messages("chat").await.unwrap();
    let stopped = messages
        .iter()
        .filter(|m| m.content.contains("was stopped before it finished"))
        .count();
    assert_eq!(stopped, 1, "{messages:?}");
    let child_status: String = sqlx::query_scalar("SELECT status FROM turns WHERE turn_id = ?")
        .bind(&child.turn_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(child_status, "interrupted");
    assert!(
        !cwd.join("made").exists(),
        "the cut-off command did not run again"
    );
}
