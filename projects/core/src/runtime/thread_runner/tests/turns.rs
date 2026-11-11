//! Turns: sending, cancelling, compaction, tools, retries and modes.

use super::*;

#[tokio::test]
async fn send_now_interrupts_and_answers_the_new_message_at_once() {
    let state = thread_state("send_now").await;
    let config = live_config("runner_config");
    let provider = provider(vec![
        vec![Step::Text("partial"), Step::Hang],
        vec![Step::Text("second"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("first").await.unwrap();
        while output.next_event().await
            != Some(ThreadEvent::Model(ModelEvent::Text("partial".into())))
        {}
        input.send_now("now").await.unwrap();
        let first = next_turn_finished(&mut output).await;
        let second = next_turn_finished(&mut output).await;
        (first.status, second.status)
    };

    let (_, statuses) = tokio::join!(runner.run(), client);
    assert_eq!(statuses, (TurnStatus::Interrupted, TurnStatus::Completed));
    assert_eq!(
        transcript(&state).await,
        [
            (MessageRole::User, "first".into()),
            (MessageRole::Assistant, "partial".into()),
            (MessageRole::User, "now".into()),
            (MessageRole::Assistant, "second".into()),
        ]
    );
}

#[tokio::test]
async fn cancel_stops_the_turn_and_waits() {
    let state = thread_state("cancel").await;
    let config = live_config("runner_config");
    let provider = provider(vec![vec![Step::Hang]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("first").await.unwrap();
        while !matches!(output.next_event().await, Some(ThreadEvent::TurnStarted(_))) {}
        input.cancel().await.unwrap();
        let turn = next_turn_finished(&mut output).await;
        let tokens = output.next_event().await;
        assert!(
            matches!(tokens, Some(ThreadEvent::Tokens { .. })),
            "{tokens:?}"
        );
        assert_eq!(output.next_event().await, Some(ThreadEvent::Idle));
        turn.status
    };

    let (_, status) = tokio::join!(runner.run(), client);
    assert_eq!(status, TurnStatus::Interrupted);
    assert_eq!(transcript(&state).await.len(), 1, "nothing was written");
}

#[tokio::test]
async fn a_message_sent_before_a_crash_is_answered_on_the_next_start() {
    let state = thread_state("pending_crash").await;
    // Confirmed to the client, but the process stopped before the thread took it in.
    state.add_pending("thread", "still there?").await.unwrap();
    let config = live_config("runner_config");
    let provider = provider(vec![vec![Step::Text("yes"), Step::Done]]);
    let (runner, input, _output) = runner(&state, &provider, &config, model(None));
    drop(input);

    runner.run().await;

    let transcript = transcript(&state).await;
    assert_eq!(
        transcript,
        [
            (MessageRole::User, "still there?".to_owned()),
            (MessageRole::Assistant, "yes".to_owned())
        ]
    );
    assert!(state.pending_messages("thread").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_model_past_the_output_limit_is_stopped_and_its_text_kept() {
    let state = thread_state("output_limit").await;
    let config = Arc::new(temp_live_config_with("output_limit_config", ""));
    // The third piece takes the output to 3 tokens; the stream must not be read on.
    let script = vec![
        Step::Text("abcd"),
        Step::Text("efgh"),
        Step::Text("ijkl"),
        Step::Hang,
    ];
    let provider = provider(vec![script]);
    let capped = Model {
        max_output: Some(2),
        ..model(None)
    };
    let (runner, input, mut output) = runner(&state, &provider, &config, capped);
    input.send("write a lot").await.unwrap();
    drop(input);

    runner.run().await;

    let mut reached = false;
    let mut status = None;
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::OutputLimitReached => reached = true,
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert!(reached);
    assert_eq!(status, Some(TurnStatus::Interrupted));
    let transcript = transcript(&state).await;
    assert_eq!(
        transcript[1],
        (MessageRole::Assistant, "abcdefghijkl".into())
    );
}

#[tokio::test]
async fn a_growing_context_is_compacted_within_the_turn() {
    let state = thread_state("compact_auto").await;
    let config = live_config("runner_config");
    let long = "a tool result long enough to push the context past the limit, \
                and longer still, since the mode's prompt is in the context too, \
                so it takes a few more words than before to get there";
    let provider = provider(vec![
        vec![Step::Call(long), Step::Done],
        vec![Step::Text("used the tool"), Step::Done],
        vec![Step::Text("done"), Step::Done],
    ]);
    // About 50 tokens before the step, the prompt included, and about 130 after it:
    // past 80% of 120, within 120 only after the summary.
    let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(120)));
    input.send("go").await.unwrap();
    drop(input);

    runner.run().await;

    let mut compaction = None;
    let mut status = None;
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::CompactionFinished(finished) => compaction = Some(finished),
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert!(compaction.is_some(), "compacted on its own");
    assert_eq!(status, Some(TurnStatus::Completed));
    let requests = provider.requests.lock().unwrap();
    assert_eq!(
        requests[2].len(),
        2,
        "the mode's prompt and the summary are left"
    );
    assert!(matches!(requests[2][0], ChatMessage::System(_)));
}

/// Like a subagent writing back while the asking turn runs a tool.
struct WriteBack(Arc<State>);

impl Tool for WriteBack {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Lets the subagent write back.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, _arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let sent = self.0.add_pending_from("thread", "found it", Some("child"));
            sent.await?;
            Ok("asked".into())
        })
    }
}

#[tokio::test]
async fn an_agent_message_joins_the_running_turn_and_starts_no_other() {
    let state = thread_state("agent_message").await;
    let child = state.create_agent_thread("child", "thread", "general", "Ember-0c");
    child.await.unwrap();
    let config = live_config("runner_config");
    let provider = provider(vec![
        vec![Step::Call("go"), Step::Done],
        vec![Step::Text("it found it"), Step::Done],
    ]);
    let tool = Box::new(WriteBack(Arc::clone(&state)));
    let (runner, input, _output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("ask the child").await.unwrap();
    drop(input);

    runner.run().await;

    let second_step = provider.requests.lock().unwrap()[1].clone();
    let header = "[message from Ember-0c, thread child]\nfound it".to_owned();
    assert_eq!(second_step.last(), Some(&ChatMessage::User(header)));
    let messages = state.list_messages("thread").await.unwrap();
    let from = messages
        .iter()
        .find(|message| message.content == "found it");
    assert_eq!(from.unwrap().from_thread_id.as_deref(), Some("child"));
    assert_eq!(
        state.count_turns("thread").await.unwrap(),
        1,
        "no second turn"
    );
}

#[tokio::test]
async fn an_answer_cut_off_keeps_what_was_written() {
    let state = thread_state("answer_cut").await;
    let config = live_config("runner_config");
    let provider = provider(vec![vec![Step::Text("half an answer"), Step::Break]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("tell me").await.unwrap();
    drop(input);

    runner.run().await;

    let mut cut = false;
    while let Some(event) = output.next_event().await {
        cut |= matches!(event, ThreadEvent::AnswerCut { .. });
    }
    assert!(cut, "the client hears why the answer stopped");
    let turn = state.list_messages("thread").await.unwrap();
    assert_eq!(
        turn.last().map(|message| message.content.as_str()),
        Some("half an answer")
    );
    assert_eq!(state.count_turns("thread").await.unwrap(), 1);
}

#[tokio::test]
async fn runs_tools_until_the_model_answers() {
    let state = thread_state("tool_loop").await;
    let config = live_config("runner_config");
    let provider = provider(vec![
        vec![Step::Call("ping"), Step::Done],
        vec![Step::Text("got ping"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("use the tool").await.unwrap();
    drop(input);

    runner.run().await;

    let mut finished = None;
    while let Some(event) = output.next_event().await {
        if let ThreadEvent::ToolFinished { result, .. } = event {
            finished = Some(result.content);
        }
    }
    assert_eq!(finished.as_deref(), Some("ping"));
    assert_eq!(
        transcript(&state).await,
        [
            (MessageRole::User, "use the tool".into()),
            (MessageRole::Assistant, String::new()),
            (MessageRole::Tool, "ping".into()),
            (MessageRole::Assistant, "got ping".into()),
        ]
    );
}

/// The first step plans and a step after a failure works it out at the effort set;
/// steps that carry out the plan think lightly.
#[tokio::test]
async fn a_turn_thinks_once_then_acts() {
    use crate::config::Effort;
    let state = thread_state("pace").await;
    let config = Arc::new(temp_live_config_with("pace_config", "effort: high\n"));
    // A call that works, one to a tool that is not there, then the answer.
    let provider = provider(vec![
        vec![Step::Call("one"), Step::Done],
        vec![Step::Tool("missing".into(), json!({})), Step::Done],
        vec![Step::Text("done"), Step::Done],
    ]);
    let (runner, input, _output) = runner(&state, &provider, &config, model(None));
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;
    assert_eq!(
        *provider.efforts.lock().unwrap(),
        [Some(Effort::High), Some(Effort::Low), Some(Effort::High)]
    );
}

/// A switch of mode applies from the next turn: the new prompt leads the request.
#[tokio::test]
async fn the_next_turn_takes_up_a_new_mode() {
    let state = thread_state("mode_switch").await;
    let config = live_config("mode_switch_config");
    let provider = provider(vec![
        vec![Step::Text("one"), Step::Done],
        vec![Step::Text("two"), Step::Done],
    ]);
    let (runner, input, _output) = runner(&state, &provider, &config, model(None));
    let modes = runner.agent.modes_dir.clone();
    input.send("first").await.unwrap();
    let running = tokio::spawn(runner.run());
    while provider.requests.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    std::fs::write(modes.join("PLAN.md"), "Plan only.").unwrap();
    config
        .update(|config| config.set_mode("PLAN".into()))
        .unwrap();
    input.send("second").await.unwrap();
    drop(input);
    running.await.unwrap();

    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests[0][0], ChatMessage::System(MAIN_PROMPT.into()));
    assert_eq!(
        requests.last().unwrap()[0],
        ChatMessage::System("Plan only.".into())
    );
}

#[tokio::test]
async fn a_passing_failure_is_retried() {
    let state = thread_state("retry").await;
    let config = live_config("retry_config");
    let provider = provider(vec![vec![Step::Fail], vec![Step::Text("ok"), Step::Done]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("hello").await.unwrap();
    drop(input);

    runner.run().await;

    let mut retried = false;
    let mut status = None;
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::Retrying { attempt, error } => {
                retried = attempt == 1 && error == "server busy";
            }
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert!(retried);
    assert_eq!(status, Some(TurnStatus::Completed));
}
