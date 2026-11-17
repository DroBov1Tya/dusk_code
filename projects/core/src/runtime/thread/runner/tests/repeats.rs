//! A model stuck on one error: the failure ladder's note, re-plan and stop in real turns,
//! and nothing of it with the switch off or when the errors differ.

use super::*;
use dusk_protocol::thread::RepeatRung;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Called as `echo`: fails with the text after `fail:`, and returns any other text.
struct Failing;

impl Tool for Failing {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Fails on request.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let text = arguments["text"].as_str().unwrap_or_default().to_owned();
            match text.strip_prefix("fail:") {
                Some(error) => Err(anyhow::anyhow!("{error}")),
                None => Ok(text),
            }
        })
    }
}

/// A step that calls the tool with `text`, then ends. Each call has its own id, as
/// ids must differ within a turn.
fn calls(text: String) -> Vec<Step> {
    vec![call_at(0, text), Step::Done]
}

/// A call of the tool with `text` at `index` of its step's calls.
fn call_at(index: u32, text: String) -> Step {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("call_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    Step::Echo(index, id, json!({ "text": text }))
}

/// The same missing file, looked for in a different place each time.
fn missing(attempt: usize) -> Vec<Step> {
    calls(format!(
        "fail:cannot open /tmp/try{attempt}/config.yml: No such file or directory"
    ))
}

const LADDER_ON: &str = "effort: high\nlimits:\n  failure_ladder: true\n";

/// Runs one turn of the chat on `scripts`; its events and how it ended.
async fn run(
    name: &str,
    settings: &str,
    scripts: Vec<Vec<Step>>,
    depth: usize,
) -> (Arc<State>, Arc<FakeProvider>, Vec<ThreadEvent>, Turn) {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let (mut runner, input, mut output) =
        runner_with(&state, &provider, &config, model(None), Box::new(Failing));
    runner.agent.depth = depth;
    input.send("find the config").await.unwrap();
    drop(input);
    runner.run().await;
    let mut events = Vec::new();
    while let Some(event) = output.next_event().await {
        events.push(event);
    }
    let turn = events
        .iter()
        .find_map(|event| match event {
            ThreadEvent::TurnFinished(turn) => Some(turn.clone()),
            _ => None,
        })
        .expect("a finished turn");
    (state, provider, events, turn)
}

fn notices(events: &[ThreadEvent]) -> Vec<(RepeatRung, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect()
}

async fn tool_results(state: &State) -> Vec<String> {
    transcript(state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::Tool)
        .map(|(_, content)| content)
        .collect()
}

#[tokio::test]
async fn the_same_error_gets_a_note_then_a_replan_then_the_turn_stops() {
    use crate::config::Effort;
    let mut scripts: Vec<_> = (1..=8).map(missing).collect();
    scripts.push(vec![Step::Text("never asked"), Step::Done]);
    let (state, provider, events, turn) = run("ladder", LADDER_ON, scripts, 0).await;

    use RepeatRung::{Replan, SameError, Stop};
    let expected = [
        (SameError, 3),
        (SameError, 4),
        (Replan, 5),
        (Replan, 6),
        (Replan, 7),
        (Stop, 8),
    ];
    assert_eq!(notices(&events), expected);
    let results = tool_results(&state).await;
    assert_eq!(results.len(), 8);
    assert!(!results[1].contains("[this same error"), "{}", results[1]);
    assert!(
        results[2].contains("Stop varying the command"),
        "{}",
        results[2]
    );
    assert!(
        results[2].contains("- echo {\"text\":\"fail:cannot open /tmp/try1/config.yml"),
        "{}",
        results[2]
    );
    assert!(
        results[4].contains("the approach is not working"),
        "{}",
        results[4]
    );
    assert!(results[7].contains("the turn stops here"), "{}", results[7]);
    // The step after the re-plan runs at the effort set, and no step follows the stop.
    let efforts = provider.efforts.lock().unwrap().clone();
    assert_eq!(efforts.len(), 8, "no model call after the stop");
    assert_eq!(efforts[5], Some(Effort::High));
    assert_eq!(turn.status, TurnStatus::Failed);
    let why = turn.error_message.unwrap_or_default();
    assert!(why.contains("came back 8 times"), "{why}");
    assert!(why.contains("No such file or directory"), "{why}");
    assert!(
        events.contains(&ThreadEvent::Failed(why)),
        "the user is told"
    );
}

#[tokio::test]
async fn a_read_between_failures_does_not_hide_the_loop() {
    let mut scripts = Vec::new();
    for attempt in 1..=3 {
        scripts.push(missing(attempt));
        scripts.push(calls(format!("read notes {attempt}")));
    }
    scripts.push(vec![Step::Text("gave up"), Step::Done]);
    let (_, _, events, turn) = run("ladder_alternating", LADDER_ON, scripts, 0).await;
    assert_eq!(notices(&events), [(RepeatRung::SameError, 3)]);
    assert_eq!(turn.status, TurnStatus::Completed);
}

#[tokio::test]
async fn errors_that_differ_never_climb() {
    let names = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    let mut scripts: Vec<_> = names
        .iter()
        .map(|name| calls(format!("fail:no module named '{name}'")))
        .collect();
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (_, _, events, turn) = run("ladder_differ", LADDER_ON, scripts, 0).await;
    assert_eq!(notices(&events), []);
    assert_eq!(turn.status, TurnStatus::Completed);
}

#[tokio::test]
async fn with_the_switch_off_the_turn_runs_as_before() {
    let mut scripts: Vec<_> = (1..=8).map(missing).collect();
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (state, provider, events, turn) = run("ladder_off", "effort: high\n", scripts, 0).await;
    assert_eq!(notices(&events), []);
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(provider.efforts.lock().unwrap().len(), 9);
    for result in tool_results(&state).await {
        assert!(!result.contains("[this same error"), "{result}");
    }
}

#[tokio::test]
async fn the_same_call_notice_is_an_event_with_the_switch_off() {
    let same = || calls("fail:cannot open /tmp/a/config.yml: No such file".into());
    let scripts = vec![same(), same(), same(), vec![Step::Text("done"), Step::Done]];
    let (state, _, events, _) = run("ladder_same_call", "", scripts, 0).await;
    assert_eq!(notices(&events), [(RepeatRung::SameCall, 3)]);
    let results = tool_results(&state).await;
    assert!(
        results[2].contains("[this exact call has returned"),
        "{}",
        results[2]
    );
}

/// Once the same error came back too often, the rest of the step's calls are not run:
/// each gets the result a stop by the user gives them.
#[tokio::test]
async fn the_calls_after_the_stop_are_not_run() {
    let mut scripts: Vec<_> = (1..=7).map(missing).collect();
    let fail = "fail:cannot open /tmp/try8/config.yml: No such file or directory";
    scripts.push(vec![
        call_at(0, fail.into()),
        call_at(1, "fail:something else".into()),
        call_at(2, "read notes".into()),
        Step::Done,
    ]);
    let (state, _, events, turn) = run("ladder_batch", LADDER_ON, scripts, 0).await;
    assert_eq!(turn.status, TurnStatus::Failed);
    let started = events
        .iter()
        .filter(|event| matches!(event, ThreadEvent::ToolStarted(_)))
        .count();
    assert_eq!(started, 8, "one call of the last step ran");
    let results = tool_results(&state).await;
    assert_eq!(results.len(), 10);
    assert!(results[7].contains("the turn stops here"), "{}", results[7]);
    for skipped in &results[8..] {
        assert_eq!(skipped, "not run: the user interrupted the turn");
    }
}

/// The stop of an agent ends its turn as failed, which `Launcher::report` passes to the
/// asker as `failed: ...`.
#[tokio::test]
async fn an_agent_that_stops_ends_its_turn_as_failed() {
    let scripts: Vec<_> = (1..=8).map(missing).collect();
    let (_, _, events, turn) = run("ladder_agent", LADDER_ON, scripts, 1).await;
    assert_eq!(turn.status, TurnStatus::Failed);
    assert!(
        turn.error_message
            .as_deref()
            .is_some_and(|why| why.starts_with("the agent stopped: the same error came back 8")),
        "{:?}",
        turn.error_message
    );
    assert_eq!(notices(&events).last(), Some(&(RepeatRung::Stop, 8)));
}

/// A failed turn would fail again: the goal pauses instead of starting another round.
#[tokio::test]
async fn a_goal_pauses_after_the_stop_instead_of_another_round() {
    let state = thread_state("ladder_goal").await;
    let settings = format!("{LADDER_ON}  max_goal_rounds: 3\n");
    let config = Arc::new(temp_live_config_with("ladder_goal_config", &settings));
    let draft = state.set_goal("thread", "find it").await.unwrap();
    let criterion = crate::state::GoalCriterion {
        id: "result".into(),
        description: "the config is found".into(),
        check: crate::state::GoalCheck::UserConfirmation,
    };
    let propose = state.propose_goal("thread", &draft.revision, vec![criterion]);
    let draft = propose.await.unwrap();
    let mut scripts: Vec<_> = (1..=8).map(missing).collect();
    scripts.push(vec![Step::Text("another round"), Step::Done]);
    let provider = provider(scripts);
    let (runner, input, mut output) =
        runner_with(&state, &provider, &config, model(None), Box::new(Failing));
    let client = async move {
        input
            .send(format!("/goal approve {}", draft.revision))
            .await
            .unwrap();
        loop {
            if let Some(ThreadEvent::Goal {
                goal: Some(goal), ..
            }) = output.next_event().await
                && goal.status == GoalStatus::Paused
                && goal.approved
            {
                return goal;
            }
        }
    };
    let (_, goal) = tokio::join!(runner.run(), client);
    assert_eq!(goal.note.as_deref(), Some("the last turn failed"));
    assert_eq!(goal.rounds, 0);
    assert_eq!(state.count_turns("thread").await.unwrap(), 1);
    assert_eq!(provider.efforts.lock().unwrap().len(), 8);
}
