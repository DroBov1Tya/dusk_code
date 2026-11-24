//! Tool calls a model wrote as text, in its answer or its reasoning.

use super::*;

#[tokio::test]
async fn a_call_left_in_the_reasoning_runs_and_leaves_the_thought() {
    let state = thread_state("reasoning_call").await;
    let config = live_config("reasoning_call_config");
    let provider = provider(vec![
        vec![
            // Closers missing, as models write them.
            Step::Reasoning(
                "Search first.\n<tool_call> <function=echo> <parameter=text> hi </tool_call>",
            ),
            Step::Done,
        ],
        vec![Step::Text("found"), Step::Done],
    ]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("look").await.unwrap();
    drop(input);
    runner.run().await;
    assert_eq!(
        next_turn_finished(&mut output).await.status,
        TurnStatus::Completed
    );
    let messages = state.list_messages("thread").await.unwrap();
    let called = &messages[2];
    assert_eq!(called.reasoning.as_deref(), Some("Search first."));
    assert_eq!(called.tool_calls[0].name, "echo");
    assert_eq!(called.tool_calls[0].arguments, json!({ "text": "hi" }));
    assert_eq!(
        transcript(&state).await.last().unwrap(),
        &(MessageRole::Assistant, "found".to_owned())
    );
}

#[tokio::test]
async fn a_call_to_an_unknown_tool_gets_a_note_a_bounded_number_of_times() {
    let state = thread_state("stranded_call").await;
    let config = live_config("stranded_call_config");
    let stranded = vec![
        Step::Reasoning("<tool_call> <function=ecko> <parameter=text> hi </tool_call>"),
        Step::Done,
    ];
    let provider = provider(vec![stranded.clone(), stranded.clone(), stranded]);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("look").await.unwrap();
    drop(input);
    runner.run().await;
    // Nothing was answered and nothing ran.
    let finished = next_turn_finished(&mut output).await;
    assert_eq!(finished.status, TurnStatus::Failed);
    assert_eq!(
        finished.error_message.as_deref(),
        Some("the model kept writing tool calls as text that could not run")
    );
    let note = ChatMessage::System(
        "You wrote a tool call as text, so it did not run. There is no tool named ecko; \
         the closest is echo. Call a tool through the tool interface, not as text."
            .into(),
    );
    let requests = provider.requests.lock().unwrap().clone();
    assert_ne!(requests[0].last(), Some(&note));
    assert_eq!(requests[1].last(), Some(&note));
    assert_eq!(requests[2].last(), Some(&note));
    // Two notes, then the turn ends instead of asking again.
    assert_eq!(provider.efforts.lock().unwrap().len(), 3);
}
