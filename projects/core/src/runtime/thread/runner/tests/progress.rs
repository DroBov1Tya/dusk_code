//! Work that brings nothing new in real turns: a turn of steps that change nothing gets a
//! note and goes on, and a goal whose rounds leave the files and tests as they were pauses
//! for the user; work that moves on gets none. Rounds that call no tool are the
//! evaluator's stall rule (`goals.rs`), and measured only there.

use super::*;
use crate::project::ledger::Edit;
use dusk_protocol::project::EditKind;
use dusk_protocol::thread::RepeatRung;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Called as `run_command`: exits with `exit`, writes `output`, and records the file
/// `edit` with the content hash `hash` in the change ledger, as the real command's
/// watch would.
struct Commands {
    state: Arc<State>,
}

impl Tool for Commands {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Runs a scripted command.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, _arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async { Ok(String::new()) })
    }

    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, crate::tools::ToolResult> {
        Box::pin(async move {
            let exit = i32::try_from(arguments["exit"].as_i64().unwrap_or(0))?;
            if let Some(path) = arguments["edit"].as_str() {
                let edit = Edit {
                    path: path.into(),
                    kind: EditKind::Modified,
                    size_before: Some(1),
                    size_after: Some(2),
                    hash_after: arguments["hash"].as_str().map(str::to_owned),
                    diff: None,
                };
                let call_id = context.progress.as_ref().map(|p| p.call_id.as_str());
                let project = context.scope.cwd().to_string_lossy();
                self.state
                    .record_changes("thread", call_id, &project, &[edit])
                    .await?;
            }
            let output = arguments["output"].as_str().unwrap_or("done");
            Ok(crate::tools::ToolResult {
                content: output.to_owned(),
                is_error: exit != 0,
                exit_code: Some(exit),
                artifacts: Vec::new(),
            })
        })
    }
}

/// A step that runs a command as the arguments say.
fn command(arguments: Value) -> Vec<Step> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("cmd_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    vec![Step::Named("run_command", id, arguments), Step::Done]
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

/// One edit, then `idle` commands that differ and change nothing, then an answer.
fn idle_turn(idle: usize) -> Vec<Vec<Step>> {
    let mut scripts = vec![command(
        json!({ "command": "sed -i s/a/b/ src/lib.rs", "edit": "src/lib.rs", "hash": "h1" }),
    )];
    for i in 0..idle {
        scripts.push(command(json!({ "command": format!("echo look {i}") })));
    }
    scripts.push(vec![Step::Text("stuck"), Step::Done]);
    scripts
}

async fn run_turn(
    name: &str,
    settings: &str,
    scripts: Vec<Vec<Step>>,
) -> (Arc<State>, Vec<ThreadEvent>, Turn) {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let tool = Box::new(Commands {
        state: Arc::clone(&state),
    });
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    input.send("fix it").await.unwrap();
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
    (state, events, turn)
}

#[tokio::test]
async fn steps_that_change_nothing_get_a_note_and_the_turn_goes_on() {
    let (state, events, turn) = run_turn("no_progress", "", idle_turn(13)).await;
    assert_eq!(notices(&events), [(RepeatRung::NoProgress, 12)]);
    let results: Vec<String> = transcript(&state)
        .await
        .into_iter()
        .filter(|(role, _)| *role == MessageRole::Tool)
        .map(|(_, content)| content)
        .collect();
    assert_eq!(results.len(), 14);
    assert!(
        results[13].contains("brought nothing new"),
        "{}",
        results[13]
    );
    assert!(
        !results[12].contains("brought nothing new"),
        "{}",
        results[12]
    );
    assert_eq!(turn.status, TurnStatus::Completed);
}

#[tokio::test]
async fn with_the_switch_off_no_note_comes() {
    let settings = "limits:\n  no_progress: false\n";
    let (_, events, turn) = run_turn("no_progress_off", settings, idle_turn(13)).await;
    assert_eq!(notices(&events), []);
    assert_eq!(turn.status, TurnStatus::Completed);
}

/// How a goal's run ended: paused with why, stalled by the evaluator's rule, or met.
#[derive(Debug, PartialEq)]
enum Ended {
    Paused(String),
    Stalled,
    Met,
}

/// Sets a goal for the chat and runs `scripts` until the goal pauses, stalls or is met;
/// how it ended, the chat's state and the turns run. Each round's scripts end with the
/// evaluator's answer.
async fn run_goal(name: &str, settings: &str, scripts: Vec<Vec<Step>>) -> (Ended, u64) {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(Commands {
        state: Arc::clone(&state),
    })];
    let (runner, input, mut output) =
        runner_with_tools(&state, &provider, &config, model(None), tools);
    let client = async move {
        input.send("/goal make the tests pass").await.unwrap();
        loop {
            match output.next_event().await.expect("the thread ended") {
                ThreadEvent::Goal {
                    goal: Some(goal), ..
                } if goal.status == GoalStatus::Paused => {
                    return Ended::Paused(goal.note.unwrap_or_default());
                }
                ThreadEvent::Goal { note, .. } if note.starts_with("Goal stalled") => {
                    return Ended::Stalled;
                }
                ThreadEvent::GoalEvaluated(evaluation)
                    if evaluation.verdict == crate::state::GoalVerdict::Met =>
                {
                    return Ended::Met;
                }
                _ => {}
            }
        }
    };
    let (_, ended) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    let turns = state.count_turns("thread").await.unwrap();
    (ended, turns)
}

/// The evaluator's answer that keeps the goal going.
fn not_met() -> Vec<Step> {
    vec![Step::Text("NOT_MET: the tests still fail"), Step::Done]
}

/// The evaluator's answer that ends it.
fn met() -> Vec<Step> {
    vec![Step::Text("MET: the tests pass"), Step::Done]
}

/// The first round, the chat's name the model is asked for after its first turn, and
/// the evaluator's verdict.
fn first_round(hash: &str) -> Vec<Vec<Step>> {
    let mut scripts = round(Some(hash));
    scripts.pop();
    scripts.push(vec![Step::Text("Failing parser"), Step::Done]);
    scripts.push(not_met());
    scripts
}

/// A round that runs the tests, after an edit to `hash` when one is given, then ends and
/// is judged not met.
fn round(hash: Option<&str>) -> Vec<Vec<Step>> {
    let mut steps = tests_round("test parser::reads_dates ... FAILED\ntest result: FAILED");
    if let Some(hash) = hash {
        steps.insert(
            0,
            command(
                json!({ "command": format!("apply {hash}"), "edit": "src/lib.rs", "hash": hash }),
            ),
        );
    }
    steps
}

/// A round that only runs the tests, which fail with `output`, then ends and is judged.
fn tests_round(output: &str) -> Vec<Vec<Step>> {
    vec![
        command(json!({ "command": "cargo test", "exit": 101, "output": output })),
        vec![Step::Text("not yet"), Step::Done],
        not_met(),
    ]
}

/// `rounds`, the last judged met.
fn ending_met(mut scripts: Vec<Vec<Step>>) -> Vec<Vec<Step>> {
    scripts.pop();
    scripts.push(met());
    scripts
}

#[tokio::test]
async fn a_goal_whose_rounds_change_nothing_pauses_with_why() {
    let mut scripts = first_round("h1");
    for _ in 0..8 {
        scripts.extend(round(None));
    }
    let (ended, turns) = run_goal("goal_stall", "", scripts).await;
    let Ended::Paused(note) = ended else {
        panic!("{ended:?}");
    };
    assert!(
        note.starts_with("the last 3 rounds changed nothing"),
        "{note}"
    );
    assert!(note.contains("parser::reads_dates"), "{note}");
    // The first round changed a file; the three after it changed nothing.
    assert_eq!(turns, 4);
}

#[tokio::test]
async fn a_goal_whose_rounds_keep_changing_files_runs_until_met() {
    let mut scripts = first_round("h0");
    for i in 1..6 {
        scripts.extend(round(Some(&format!("h{i}"))));
    }
    let (ended, turns) = run_goal("goal_moving", "", ending_met(scripts)).await;
    assert_eq!(ended, Ended::Met);
    assert_eq!(turns, 6);
}

#[tokio::test]
async fn with_the_switch_off_rounds_that_change_nothing_go_on() {
    let mut scripts = first_round("h1");
    for _ in 0..5 {
        scripts.extend(round(None));
    }
    let settings = "limits:\n  goal_stall: false\n";
    let (ended, turns) = run_goal("goal_stall_off", settings, ending_met(scripts)).await;
    assert_eq!(ended, Ended::Met);
    assert_eq!(turns, 6);
}

/// Rounds that call no tool are the evaluator's stall, with its own message, and never
/// the pause for rounds that changed nothing.
#[tokio::test]
async fn rounds_without_tools_after_an_edit_are_the_evaluators_stall() {
    let mut scripts = first_round("h1");
    for _ in 0..8 {
        // A look around, a question, a plan: no file changed, no test run.
        scripts.push(vec![Step::Text("read the parser docs"), Step::Done]);
        scripts.push(not_met());
    }
    let (ended, turns) = run_goal("goal_research", "", scripts).await;
    assert_eq!(ended, Ended::Stalled);
    assert_eq!(turns, 4);
}

#[tokio::test]
async fn rounds_that_fix_build_errors_are_not_a_stall() {
    let mut scripts = first_round("h1");
    // Rounds that only run the build: no test id is read from it, but fewer errors are
    // left each time.
    for left in (1..=5).rev() {
        let errors: Vec<String> = (0..left)
            .map(|i| format!("error[E0425]: cannot find value `v{i}` in this scope"))
            .collect();
        let output = format!(
            "{}\nerror: could not compile `app` due to {left} previous errors",
            errors.join("\n")
        );
        scripts.extend(tests_round(&output));
    }
    let (ended, turns) = run_goal("goal_build_errors", "", ending_met(scripts)).await;
    assert_eq!(ended, Ended::Met);
    assert_eq!(turns, 6);
}
