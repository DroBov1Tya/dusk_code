//! Work that brings nothing new in real turns: a turn of steps that change nothing gets a
//! note and goes on.

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
