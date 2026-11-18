//! The read-only calls of one step run together; their results are stored in call order,
//! and everything else keeps its order.

use super::*;
use dusk_protocol::thread::RepeatRung;

/// One step that calls `echo` once per `(text, arguments)`, under the text as call id.
fn step(calls: &[(&str, Value)]) -> Vec<Step> {
    let mut step: Vec<Step> = calls
        .iter()
        .enumerate()
        .map(|(index, (text, extra))| {
            let mut arguments = extra.clone();
            arguments["text"] = json!(text);
            Step::Echo(index as u32, (*text).into(), arguments)
        })
        .collect();
    step.push(Step::Done);
    step
}

/// A read-only call that takes `ms` to run.
fn read(ms: u64) -> Value {
    json!({ "read_only": true, "repeatable": true, "sleep_ms": ms })
}

fn write(ms: u64) -> Value {
    json!({ "read_only": false, "repeatable": false, "sleep_ms": ms })
}

/// Runs one turn whose model answers with `steps`, then "done".
async fn turn(
    name: &str,
    settings: &str,
    steps: Vec<Vec<Step>>,
    counter: &Counter,
) -> (Arc<State>, Vec<ThreadEvent>) {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let mut scripts = steps;
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let provider = provider(scripts);
    let tool = Box::new(counter.clone());
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;
    (state, drain(&mut output).await)
}

async fn results(state: &State) -> Vec<String> {
    transcript(state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::Tool)
        .map(|(_, content)| content)
        .collect()
}

/// `S` and `F` with the call id, for the tool events in the order they came.
fn tool_events(events: &[ThreadEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ToolStarted(call) => Some(format!("S{}", call.id)),
            ThreadEvent::ToolFinished { call_id, .. } => Some(format!("F{call_id}")),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn three_reads_overlap_and_are_stored_in_call_order() {
    let counter = Counter::default();
    // The first takes longest, so it ends last: the order of results is not the order of ends.
    let step = step(&[("a", read(80)), ("b", read(20)), ("c", read(40))]);
    let (state, events) = turn("together_reads", "", vec![step], &counter).await;

    let log = counter.log();
    assert_eq!(log[..3], ["start:a", "start:b", "start:c"], "{log:?}");
    assert_eq!(log[3..], ["end:b", "end:c", "end:a"], "{log:?}");
    assert_eq!(results(&state).await, ["a", "b", "c"]);
    // Clients pair the two events by call id: every start comes before its finish.
    assert_eq!(tool_events(&events), ["Sa", "Sb", "Sc", "Fa", "Fb", "Fc"]);
}

#[tokio::test]
async fn a_write_between_reads_splits_them_and_keeps_the_order() {
    let counter = Counter::default();
    let step = step(&[
        ("r1", read(30)),
        ("w", write(30)),
        ("r2", read(30)),
        ("r3", read(30)),
    ]);
    let (state, events) = turn("together_split", "", vec![step], &counter).await;

    let log = counter.log();
    assert_eq!(
        log[..4],
        ["start:r1", "end:r1", "start:w", "end:w"],
        "{log:?}"
    );
    assert_eq!(log[4..6], ["start:r2", "start:r3"], "{log:?}");
    assert_eq!(results(&state).await, ["r1", "w", "r2", "r3"]);
    let expected = ["Sr1", "Fr1", "Sw", "Fw", "Sr2", "Sr3", "Fr2", "Fr3"];
    assert_eq!(tool_events(&events), expected);
}

#[tokio::test]
async fn a_stop_mid_group_keeps_what_finished_and_gives_the_rest_a_result() {
    let counter = Counter::default();
    let step = step(&[
        ("a", read(0)),
        ("b", read(0)),
        ("slow", read(60_000)),
        ("after", write(0)),
    ]);
    let state = thread_state("together_stop").await;
    let config = live_config("together_stop_config");
    let provider = provider(vec![step, vec![Step::Text("never"), Step::Done]]);
    let tool = Box::new(counter.clone());
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("go").await.unwrap();
    let running = tokio::spawn(runner.run());
    // Both fast calls are done and the slow one runs.
    while !(counter.log().contains(&"end:b".to_owned())
        && counter.log().contains(&"end:a".to_owned()))
    {
        tokio::task::yield_now().await;
    }
    input.cancel().await.unwrap();
    drop(input);
    let events = drain(&mut output).await;
    running.await.unwrap();

    assert_eq!(
        results(&state).await,
        [
            "a",
            "b",
            "stopped: the user interrupted the turn",
            "not run: the user interrupted the turn"
        ]
    );
    assert!(!counter.log().contains(&"start:after".to_owned()));
    assert_eq!(tool_events(&events), ["Sa", "Sb", "Sslow", "Fa", "Fb"]);
}

#[tokio::test]
async fn with_the_switch_off_the_calls_run_one_by_one() {
    let counter = Counter::default();
    let step = step(&[("a", read(10)), ("b", read(10)), ("c", read(10))]);
    let off = "limits:\n  parallel_reads: false\n";
    let (state, events) = turn("together_off", off, vec![step], &counter).await;

    let expected = ["a", "b", "c"].map(|t| [format!("start:{t}"), format!("end:{t}")]);
    assert_eq!(counter.log(), expected.concat());
    assert_eq!(results(&state).await, ["a", "b", "c"]);
    assert_eq!(tool_events(&events), ["Sa", "Fa", "Sb", "Fb", "Sc", "Fc"]);
}

#[tokio::test]
async fn three_identical_reads_in_a_row_still_get_the_repeat_note() {
    let counter = Counter::default();
    // All three come in one step, so they run as one group.
    let one_step = vec![
        Step::Echo(
            0,
            "x1".into(),
            json!({ "text": "same", "read_only": true, "repeatable": true }),
        ),
        Step::Echo(
            1,
            "x2".into(),
            json!({ "text": "same", "read_only": true, "repeatable": true }),
        ),
        Step::Echo(
            2,
            "x3".into(),
            json!({ "text": "same", "read_only": true, "repeatable": true }),
        ),
        Step::Done,
    ];
    let (state, events) = turn("together_repeat", "", vec![one_step], &counter).await;

    let notices: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect();
    assert_eq!(notices, [(RepeatRung::SameCall, 3)]);
    let results = results(&state).await;
    assert!(!results[1].contains("[this exact call has returned"));
    assert!(
        results[2].contains("[this exact call has returned"),
        "{}",
        results[2]
    );
}
