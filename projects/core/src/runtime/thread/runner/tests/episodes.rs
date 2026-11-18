//! Episodes in real turns: a loop a pass ended and a verification that passed are written
//! down for a saved project, with the event, the files the ledger saw and the lesson;
//! nothing with the switch off, in an unsaved folder, or when the store fails, and the
//! turn goes on either way.

use super::*;
use crate::project::ledger::Edit;
use crate::state::{Episode, EpisodeKind};
use dusk_protocol::project::EditKind;
use dusk_protocol::thread::EpisodeKind as Recorded;
use std::sync::atomic::{AtomicUsize, Ordering};

const EPISODES_ON: &str = "limits:\n  episodes: true\n  episode_lesson: false\n";
const LESSON_ON: &str = "limits:\n  episodes: true\n  episode_lesson: true\n";
const UNKNOWN: &str = "error[E0425]: cannot find value `limit` in this scope";

/// Called as `run_command`: exits with `exit`, writes `output`, and records `edit` in
/// the change ledger of the chat `thread` as the real command's watch would.
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
                    hash_after: None,
                    diff: None,
                };
                let call_id = context.progress.as_ref().map(|p| p.call_id.as_str());
                let project = context.scope.cwd().to_string_lossy();
                self.state
                    .record_changes("thread", call_id, &project, &[edit])
                    .await?;
            }
            let output = arguments["output"].as_str().unwrap_or("ok");
            Ok(crate::tools::ToolResult {
                content: format!("exit code: {exit}\nstderr:\n{output}"),
                is_error: exit != 0,
                exit_code: Some(exit),
                artifacts: Vec::new(),
            })
        })
    }
}

/// A step that runs `command` as the arguments say, then ends.
fn run_command(arguments: Value) -> Vec<Step> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("cmd_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    vec![Step::Named("run_command", id, arguments), Step::Done]
}

fn failing(command: &str) -> Vec<Step> {
    run_command(json!({ "command": command, "exit": 101, "output": UNKNOWN }))
}

/// Fails twice on one error with an edit between, then passes, then answers `answer`.
fn a_loop(answer: &'static str) -> Vec<Vec<Step>> {
    vec![
        failing("cargo test -p core"),
        run_command(json!({ "command": "sed -i s/a/b/ src/lib.rs", "edit": "src/lib.rs" })),
        failing("cargo test -p core --lib"),
        run_command(json!({ "command": "cargo test -p core", "edit": "target/out.txt" })),
        vec![Step::Text(answer), Step::Done],
    ]
}

struct Ran {
    state: Arc<State>,
    events: Vec<ThreadEvent>,
    turn: Turn,
    /// The runner's folder, as the store names it.
    project: String,
}

impl Ran {
    fn recorded(&self) -> Vec<Recorded> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ThreadEvent::EpisodeRecorded { kind } => Some(*kind),
                _ => None,
            })
            .collect()
    }

    async fn episodes(&self) -> Vec<Episode> {
        self.state
            .list_episodes(&self.project, None, 10)
            .await
            .unwrap()
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes")
            .fetch_one(&self.state.pool)
            .await
            .unwrap()
    }
}

/// Runs one turn on `scripts`; with `saved`, the runner's folder is a saved project.
async fn run(name: &str, settings: &str, scripts: Vec<Vec<Step>>, saved: bool) -> Ran {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let tool = Box::new(Commands {
        state: Arc::clone(&state),
    });
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    let project = runner.agent.scope.cwd().to_string_lossy().into_owned();
    if saved {
        state.create_project("p", "P", "", &project).await.unwrap();
    }
    input.send("make the tests pass").await.unwrap();
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
    Ran {
        state,
        events,
        turn,
        project,
    }
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
async fn a_loop_that_passed_is_written_down_with_what_changed() {
    let ran = run("episode_fixed", EPISODES_ON, a_loop("fixed"), true).await;
    assert_eq!(ran.turn.status, TurnStatus::Completed);
    assert_eq!(ran.recorded(), [Recorded::Fixed]);
    let episodes = ran.episodes().await;
    assert_eq!(episodes.len(), 1);
    let episode = &episodes[0];
    assert_eq!(episode.kind, EpisodeKind::Fixed);
    assert_eq!(episode.tool, "run_command");
    assert_eq!(episode.model, "fake");
    assert_eq!(episode.turn_id.as_deref(), Some(ran.turn.turn_id.as_str()));
    assert!(episode.problem.contains("cannot find value"), "{episode:?}");
    assert_eq!(episode.failed, "cargo test -p core --lib");
    // The pass's own change is not what led to it.
    assert_eq!(
        episode.worked,
        "cargo test -p core after changes to src/lib.rs"
    );
    assert!(episode.lesson.is_empty());
    assert!(ran.state.has_episodes(&ran.project), "recall is offered");
    for result in tool_results(&ran.state).await {
        assert!(!result.contains("[this passed after"), "{result}");
    }
}

#[tokio::test]
async fn the_lesson_line_is_asked_for_and_stored_cleaned() {
    let answer = "All green.\nLesson: first try\nLesson: the token=ghp_abcdefghijklmnopqrstuvwxyz0123456789 was stale";
    let ran = run("episode_lesson", LESSON_ON, a_loop(answer), true).await;
    let results = tool_results(&ran.state).await;
    let note = "[this passed after the same error came back 2 times. End your answer with \
                one line: Lesson: <what was wrong, under 20 words>]";
    assert!(
        results[3].ends_with(&format!("\n\n{note}")),
        "{}",
        results[3]
    );
    for result in &results[..3] {
        assert!(!result.contains("[this passed after"), "{result}");
    }
    let lesson = ran.episodes().await[0].lesson.clone();
    assert!(lesson.starts_with("the token"), "{lesson}");
    assert!(lesson.contains("<redacted>"), "{lesson}");
    assert!(!lesson.contains("ghp_"), "{lesson}");
}

#[tokio::test]
async fn an_answer_without_a_lesson_line_stores_none() {
    let ran = run("episode_no_lesson", LESSON_ON, a_loop("All green."), true).await;
    assert_eq!(ran.recorded(), [Recorded::Fixed]);
    assert!(ran.episodes().await[0].lesson.is_empty());
}

#[tokio::test]
async fn with_the_lesson_off_nothing_is_asked_or_stored() {
    let ran = run(
        "episode_lesson_off",
        EPISODES_ON,
        a_loop("Lesson: kept?"),
        true,
    )
    .await;
    for result in tool_results(&ran.state).await {
        assert!(!result.contains("[this passed after"), "{result}");
    }
    assert!(ran.episodes().await[0].lesson.is_empty());
}

#[tokio::test]
async fn with_the_switch_off_nothing_is_written() {
    let ran = run(
        "episode_off",
        "limits:\n  episodes: false\n",
        a_loop("Lesson: x"),
        true,
    )
    .await;
    assert_eq!(ran.turn.status, TurnStatus::Completed);
    assert_eq!(ran.recorded(), []);
    assert_eq!(ran.rows().await, 0);
    assert!(!ran.state.has_episodes(&ran.project));
}

#[tokio::test]
async fn an_unsaved_folder_gets_no_episode_and_no_note() {
    let ran = run("episode_unsaved", LESSON_ON, a_loop("Lesson: x"), false).await;
    assert_eq!(ran.turn.status, TurnStatus::Completed);
    assert_eq!(ran.recorded(), []);
    assert_eq!(ran.rows().await, 0);
    for result in tool_results(&ran.state).await {
        assert!(!result.contains("[this passed after"), "{result}");
    }
}

#[tokio::test]
async fn a_store_that_fails_does_not_fail_the_turn() {
    let state = thread_state("episode_broken").await;
    sqlx::query("DROP TABLE episodes")
        .execute(&state.pool)
        .await
        .unwrap();
    let config = Arc::new(temp_live_config_with("episode_broken_config", LESSON_ON));
    let provider = provider(a_loop("Lesson: x"));
    let tool = Box::new(Commands {
        state: Arc::clone(&state),
    });
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    let project = runner.agent.scope.cwd().to_string_lossy().into_owned();
    state.create_project("p", "P", "", &project).await.unwrap();
    input.send("make the tests pass").await.unwrap();
    drop(input);
    runner.run().await;
    let events = drain(&mut output).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::EpisodeRecorded { .. }))
    );
    let finished = events.iter().find_map(|event| match event {
        ThreadEvent::TurnFinished(turn) => Some(turn.status),
        _ => None,
    });
    assert_eq!(finished, Some(TurnStatus::Completed));
    let results = tool_results(&state).await;
    assert_eq!(results.len(), 4);
    assert!(!results[3].contains("[this passed after"), "{}", results[3]);
}

#[tokio::test]
async fn a_verification_that_passed_is_written_down() {
    let state = thread_state("episode_verified").await;
    let (goal, result_id) = super::goals::verified_fixture(&state).await;
    let config = Arc::new(temp_live_config_with(
        "episode_verified_config",
        EPISODES_ON,
    ));
    let provider = provider(vec![
        vec![
            Step::Tool(
                "goal".into(),
                json!({"action":"request_verification","revision":goal.revision,
                    "evidence":[{"criterion_id":"exit","result_id":result_id}]}),
            ),
            Step::Done,
        ],
        vec![Step::Text("I think it is done"), Step::Done],
        vec![Step::Text("thread name"), Step::Done],
    ]);
    let tool = Box::new(crate::tools::GoalTool::new(
        Arc::clone(&state),
        "thread".into(),
    ));
    let (runner, input, mut output) = runner_with(&state, &provider, &config, model(None), tool);
    let project = runner.agent.scope.cwd().to_string_lossy().into_owned();
    state.create_project("p", "P", "", &project).await.unwrap();
    let client = async move {
        input.send("finish the goal").await.unwrap();
        let mut recorded = Vec::new();
        while let Some(event) = output.next_event().await {
            match event {
                ThreadEvent::EpisodeRecorded { kind } => recorded.push(kind),
                ThreadEvent::Goal {
                    goal: Some(goal), ..
                } if goal.status == GoalStatus::Done => return recorded,
                _ => {}
            }
        }
        panic!("the goal was not verified");
    };
    let (_, recorded) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(), client)
    })
    .await
    .unwrap();
    assert_eq!(recorded, [Recorded::Verified]);
    let episodes = state.list_episodes(&project, None, 10).await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].kind, EpisodeKind::Verified);
    assert_eq!(episodes[0].problem, "build it");
    assert_eq!(episodes[0].worked, "exit 0");
    assert!(state.has_episodes(&project));
}
