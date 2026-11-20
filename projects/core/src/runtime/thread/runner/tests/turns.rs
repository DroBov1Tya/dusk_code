//! Turns: sending, cancelling, compaction, tools, retries and modes.

use super::*;

#[tokio::test]
async fn a_chat_is_automatically_named_only_on_its_first_turn() {
    let state = thread_state("auto_title_once").await;
    let config = live_config("auto_title_once_config");
    let provider = provider(vec![
        vec![Step::Text("first answer"), Step::Done],
        vec![Step::Text("Generated title"), Step::Done],
        vec![Step::Text("second answer"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("first question").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
        while output.next_event().await.expect("thread is open") != ThreadEvent::Idle {}
        input.send("second question").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
    };
    tokio::join!(runner.run(), client);

    let thread = state.load_thread("thread").await.unwrap().unwrap();
    assert_eq!(thread.title.as_deref(), Some("Generated title"));
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
}

/// The core counts what a turn spent: after each model call its totals so far, and the
/// same totals on the finished turn, so no client sums usage.
#[tokio::test]
async fn a_turn_reports_the_sum_of_its_model_calls() {
    let state = thread_state("turn_progress").await;
    let config = live_config("turn_progress_config");
    let provider = provider(vec![
        vec![Step::Call("one"), Step::Usage(100, 20, 5), Step::Done],
        vec![Step::Text("done"), Step::Usage(300, 40, 7), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("go").await.unwrap();
        let mut progress = Vec::new();
        loop {
            match output.next_event().await.expect("thread is open") {
                ThreadEvent::TurnProgress {
                    steps,
                    tokens,
                    cost_micro_usd,
                    ..
                } => progress.push((steps, tokens, cost_micro_usd)),
                ThreadEvent::TurnFinished(turn) => {
                    assert_eq!(progress, [(1, 120, 5), (2, 460, 12)]);
                    assert_eq!((turn.steps, turn.tokens, turn.cost_micro_usd), (2, 460, 12));
                    break;
                }
                _ => {}
            }
        }
    };
    tokio::join!(runner.run(), client);
}

#[tokio::test]
async fn undo_does_not_retry_a_failed_title_request() {
    let state = thread_state("failed_title_undo").await;
    let config = live_config("failed_title_undo_config");
    let provider = provider(vec![
        vec![Step::Text("first answer"), Step::Done],
        vec![Step::Reject],
        vec![Step::Text("replacement answer"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let read_state = Arc::clone(&state);
    let client = async move {
        input.send("first question").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
        while output.next_event().await.expect("thread is open") != ThreadEvent::Idle {}
        let points = read_state.undo_points("thread").await.unwrap();
        input
            .send(&format!("/undo {}", points[0].sequence))
            .await
            .unwrap();
        loop {
            match output.next_event().await.expect("thread is open") {
                ThreadEvent::Undone { text } => {
                    assert_eq!(text, "first question");
                    break;
                }
                ThreadEvent::Rejected { reason } => panic!("{reason}"),
                _ => {}
            }
        }
        input.send("replacement question").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
    };
    tokio::join!(runner.run(), client);

    assert_eq!(state.count_turns("thread").await.unwrap(), 1);
    let thread = state.load_thread("thread").await.unwrap().unwrap();
    assert_eq!(thread.title.as_deref(), Some("first question"));
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_manually_named_chat_needs_no_title_request() {
    let state = thread_state("manual_title").await;
    state.set_title("thread", "first question").await.unwrap();
    let config = live_config("manual_title_config");
    let provider = provider(vec![vec![Step::Text("first answer"), Step::Done]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let client = async move {
        input.send("first question").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
    };
    tokio::join!(runner.run(), client);

    let thread = state.load_thread("thread").await.unwrap().unwrap();
    assert_eq!(thread.title.as_deref(), Some("first question"));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}

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
async fn provider_tokens_are_not_recounted_as_four_byte_chunks() {
    let state = thread_state("output_tokenizer").await;
    let config = live_config("output_tokenizer_config");
    let provider = provider(vec![vec![
        Step::Reasoning("Long reasoning does not measure provider output tokens."),
        Step::Text("Ответ на русском языке.\n```rust\nlet result = 42;\n```"),
        Step::Done,
    ]]);
    let model = Model {
        max_output: Some(2),
        ..model(None)
    };
    let (runner, input, mut output) = runner(&state, &provider, &config, model);
    input.send("write a lot").await.unwrap();
    drop(input);
    runner.run().await;
    let mut status = None;
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::OutputLimitReached => panic!("provider finished normally"),
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert_eq!(status, Some(TurnStatus::Completed));
    assert_eq!(
        transcript(&state).await[1].1,
        "Ответ на русском языке.\n```rust\nlet result = 42;\n```"
    );
    assert_eq!(*provider.output_limits.lock().unwrap(), [Some(2)]);
}

#[tokio::test]
async fn genuine_provider_limit_keeps_partial_text_and_refuses_incomplete_tool() {
    let state = thread_state("provider_length").await;
    let config = live_config("provider_length_config");
    let provider = provider(vec![vec![
        Step::Text("Part of the answer."),
        Step::PartialCall("{\"text\":\"unfinished"),
        Step::Length,
    ]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;

    let mut reached = false;
    let mut status = None;
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::OutputLimitReached => reached = true,
            ThreadEvent::ToolStarted(_) => panic!("unfinished tool must not run"),
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert!(reached);
    assert_eq!(status, Some(TurnStatus::Interrupted));
    assert_eq!(transcript(&state).await[1].1, "Part of the answer.");
    assert_eq!(*provider.output_limits.lock().unwrap(), [None]);
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
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("ask the child").await.unwrap();
    drop(input);

    runner.run().await;

    // Mid-turn the message is the agent's: it names nothing and is no user message.
    let events = drain(&mut output).await;
    let titles: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TitleChanged { title } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["ask the child"]);
    assert!(events.iter().any(
        |event| matches!(event, ThreadEvent::FromAgent(message) if message.content == "found it")
    ));
    assert!(!events.iter().any(
        |event| matches!(event, ThreadEvent::UserMessage(message) if message.content == "found it")
    ));

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

/// Between turns the user's message names the thread, reaches the clients as theirs and
/// ends the goal's wait for them.
#[tokio::test]
async fn a_message_between_turns_names_the_thread_and_ends_the_goals_wait() {
    let state = thread_state("between_turns").await;
    state.set_goal("thread", "build it").await.unwrap();
    sqlx::query("UPDATE goals SET awaiting_user = 1")
        .execute(&state.pool)
        .await
        .unwrap();
    let config = live_config("runner_config");
    let answer = vec![Step::Text("ok"), Step::Done];
    let provider = provider(vec![answer.clone(), answer]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("hello there").await.unwrap();
    drop(input);

    runner.run().await;

    let events = drain(&mut output).await;
    assert!(events.iter().any(
        |event| matches!(event, ThreadEvent::TitleChanged { title } if title == "hello there")
    ));
    assert!(events.iter().any(
        |event| matches!(event, ThreadEvent::UserMessage(message) if message.content == "hello there")
    ));
    let waiting: bool =
        sqlx::query_scalar("SELECT awaiting_user FROM goals WHERE thread_id = 'thread'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(!waiting);
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

/// A block lives in the stored call: a client that reopens the thread reads the same
/// block the live events gave, and `read_history` gives its plain text.
#[tokio::test]
async fn a_shown_block_is_read_back_from_the_stored_messages() {
    use crate::tools::{ReadHistory, Show};
    use dusk_protocol::thread::Block;

    let state = thread_state("shown_block").await;
    let config = Arc::new(temp_live_config_with(
        "shown_block_config",
        "tools:\n  show: true\n",
    ));
    let arguments = json!({ "choices": { "labels": ["Yes, fix it", "Not now"] } });
    let provider = provider(vec![
        vec![Step::Tool("show".into(), arguments), Step::Done],
        vec![Step::Text("Shall I?"), Step::Done],
    ]);
    let (runner, input, mut output) =
        runner_with(&state, &provider, &config, model(None), Box::new(Show));
    input.send("fix it?").await.unwrap();
    drop(input);

    runner.run().await;

    let (mut started, mut live) = (None, None);
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::ToolStarted(call) => started = Some(call),
            ThreadEvent::ToolFinished { result, .. } => {
                assert!(!result.is_error, "{}", result.content);
                live = Block::shown(started.as_ref().unwrap(), &result.content);
            }
            _ => {}
        }
    }
    let text = "Choices:\n1. Yes, fix it\n2. Not now";
    assert_eq!(live.as_ref().map(Block::text).as_deref(), Some(text));

    let messages = state.list_messages("thread").await.unwrap();
    let call = messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .next()
        .expect("the call is stored with the assistant message");
    let answer = messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some(call.id.as_str()))
        .expect("the call has its tool message");
    assert_eq!(Block::shown(call, &answer.content), live);

    let root = temp_dir("shown_block_scope");
    std::fs::create_dir_all(&root).unwrap();
    let context = crate::testing::tool_context(&ToolScope::new(&root, &root).unwrap());
    let history = ReadHistory::new(Arc::clone(&state), "thread".into());
    let read = history.call(json!({}), context).await.unwrap();
    assert!(read.contains(text), "{read}");
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
            ThreadEvent::Retrying { attempt, error, .. } => {
                retried = attempt == 1 && error == "server busy";
            }
            ThreadEvent::TurnFinished(turn) => status = Some(turn.status),
            _ => {}
        }
    }
    assert!(retried);
    assert_eq!(status, Some(TurnStatus::Completed));
}

#[tokio::test]
async fn the_context_report_agrees_with_the_tokens_event() {
    let state = thread_state("context_agrees").await;
    let config = live_config("runner_config");
    let provider = provider(vec![vec![Step::Text("hi"), Step::Done]]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(Some(50_000)));
    // What `Runtime::context` counts a running thread with.
    let source = runner.mailbox(&input).context;
    let client = async move {
        // Before the first step the tool specs are estimated; after it the turn's count leads.
        for sent in [false, true] {
            if sent {
                input.send("hello").await.unwrap();
            }
            let mut reported = None;
            loop {
                match output.next_event().await.expect("the thread ended") {
                    ThreadEvent::Tokens { tokens, limit } => {
                        reported = Some((tokens, limit));
                        // Nothing follows the count of a thread that has not been written to.
                        if !sent {
                            break;
                        }
                    }
                    ThreadEvent::Idle => break,
                    _ => {}
                }
            }
            let (tokens, limit) = reported.expect("no tokens were reported");
            let report = commands::context_report(
                &state,
                Some("thread"),
                "main",
                None,
                &source.specs(),
                source.limit(),
                &commands::ContextDetail::default(),
            )
            .await
            .unwrap();
            assert_eq!((report.total, report.limit), (tokens.context, limit));
            assert_eq!(limit, 50_000);
        }
    };
    tokio::join!(runner.run(), client);
}

/// A source goes in full with the one model call that read it: the next step, here a
/// failed one, and the next turn name it in a line instead.
#[tokio::test]
async fn a_source_read_once_is_not_sent_again() {
    let state = thread_state("source_once").await;
    let config = live_config("runner_config");
    let provider = provider(vec![
        vec![Step::Call("ping"), Step::Done],
        vec![Step::Reject],
        // The thread's title, asked for after its first turn.
        vec![Step::Text("Title"), Step::Done],
        vec![Step::Text("ok"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    let sources = crate::message::sources::ThreadSources {
        state: &state,
        workspace: runner.agent.scope.workspace(),
        thread_id: "thread",
    };
    let store = ArtifactStore::new(runner.agent.scope.workspace().join("artifacts"));
    let message = sources
        .keep_pastes(&dusk_protocol::thread::wrap("SOURCE BODY"), &store)
        .await
        .unwrap();
    let client = async move {
        input.send(&message).await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Failed
        );
        input.send("again").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
    };
    tokio::join!(runner.run(), client);

    let requests = provider.requests.lock().unwrap();
    let full = |messages: &Vec<ChatMessage>| {
        messages.iter().any(
            |message| matches!(message, ChatMessage::User(text) if text.contains("SOURCE BODY")),
        )
    };
    assert!(full(&requests[0]), "the first step carries it");
    assert!(!full(&requests[1]), "the next step does not");
    assert!(!full(requests.last().unwrap()), "the next turn does not");
}
