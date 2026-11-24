//! Text that goes round in real turns: the stream is cut and the step taken again with
//! a note, twice at most, then it streams whole; long answers that repeat by their
//! nature (code, lists) stream whole.

use super::*;
use dusk_protocol::thread::RepeatRung;

fn notices(events: &[ThreadEvent]) -> Vec<(RepeatRung, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect()
}

/// `text` streamed in parts of 40 bytes, as a provider sends it.
fn streamed(text: String) -> Vec<Step> {
    let text: &'static str = Box::leak(text.into_boxed_str());
    let mut steps = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.floor_char_boundary(40.min(rest.len())).max(1);
        let (part, after) = rest.split_at(end);
        steps.push(Step::Text(part));
        rest = after;
    }
    steps.push(Step::Done);
    steps
}

fn going_round() -> Vec<Step> {
    streamed(format!(
        "First I read the parser.\n{}",
        "I will check the parser for the error once more now. ".repeat(40)
    ))
}

async fn run(
    name: &str,
    settings: &str,
    scripts: Vec<Vec<Step>>,
) -> (Arc<State>, Arc<FakeProvider>, Vec<ThreadEvent>, Turn) {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("why does the parser fail?").await.unwrap();
    drop(input);
    runner.run().await;
    let events = drain(&mut output).await;
    let turn = events
        .iter()
        .find_map(|event| match event {
            ThreadEvent::TurnFinished(turn) => Some(turn.clone()),
            _ => None,
        })
        .expect("a finished turn");
    (state, provider, events, turn)
}

async fn answer(state: &State) -> String {
    transcript(state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::Assistant)
        .map(|(_, content)| content)
        .next_back()
        .unwrap_or_default()
}

#[tokio::test]
async fn text_going_round_is_cut_and_the_step_taken_again_with_a_note() {
    let scripts = vec![
        going_round(),
        vec![Step::Text("The date format."), Step::Done],
    ];
    let (state, provider, events, turn) = run("chant_once", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(notices(&events), [(RepeatRung::Cycle, 10)]);
    assert_eq!(answer(&state).await, "The date format.");
    let requests = provider.requests.lock().unwrap().clone();
    let again = &requests[1];
    assert!(
        matches!(again.last(), Some(ChatMessage::System(note)) if note.contains("was cut")),
        "{again:?}"
    );
}

#[tokio::test]
async fn after_two_cuts_the_text_streams_whole_and_the_turn_goes_on() {
    // A rule may read healthy text as a loop: it never ends the turn or cuts its calls.
    let scripts = vec![going_round(), going_round(), going_round()];
    let (state, _, events, turn) = run("chant_always", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(notices(&events).len(), 2);
    let kept = answer(&state).await;
    assert!(kept.starts_with("First I read the parser."), "{kept}");
    assert_eq!(kept.matches("once more").count(), 40, "{kept}");
}

#[tokio::test]
async fn long_code_and_lists_stream_whole() {
    let code = format!(
        "Here are the cases:\n```rust\n{}```\nAnd the steps:\n{}",
        "    assert_eq!(parse(\"2024-01-01\"), Ok(date(2024, 1, 1)));\n".repeat(40),
        "- run the parser on the same file again and compare\n".repeat(40)
    );
    let (state, _, events, turn) = run("chant_code", "", vec![streamed(code.clone())]).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(notices(&events), []);
    assert_eq!(answer(&state).await, code);
}

#[tokio::test]
async fn with_the_switch_off_the_text_streams_whole() {
    let settings = "limits:\n  text_loops: false\n";
    let (state, _, events, turn) = run("chant_off", settings, vec![going_round()]).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(notices(&events), []);
    assert_eq!(answer(&state).await.matches("once more").count(), 40);
}
