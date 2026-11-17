//! A turn a kill cut off goes on after the restart, and nothing that may have taken effect
//! runs twice. Each test builds the database as the kill left it, or kills a runner, then
//! starts a second runner on the same state. `cuts` holds a test for each place a kill
//! can fall; `decisions` what the user answers when asked.

mod cuts;
mod decisions;

use super::*;
use crate::state::TurnAnswer;
use dusk_protocol::thread::TurnWait;
use dusk_protocol::tools::Question;

const LOST: &str = "no result: the process stopped while this call ran, so it may have taken \
     effect and may still be running";
const NOT_RUN: &str = "not run: an earlier call of this step was cut off by a restart";
const CHECK_NOTE: &str = "The cut-off call may have taken effect and may still be running. \
     Before repeating it, check whether it took effect; before repeating a command, check \
     whether a process it started is still running.";

/// The database as a kill in the middle of a step left it: the turn runs, its step
/// called `echo` once per text under the text as its id, and the first `done` calls
/// have their results.
async fn killed_in_step(state: &State, texts: &[&str], done: usize) -> Turn {
    state.add_user_message("thread", "go", "go").await.unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let calls = texts
        .iter()
        .map(|text| ToolCall {
            id: (*text).into(),
            name: "echo".into(),
            arguments: json!({ "text": text }),
        })
        .collect();
    let answer = TurnAnswer {
        content: String::new(),
        reasoning: None,
    };
    let id = &turn.turn_id;
    state.record_tool_calls(id, answer, calls).await.unwrap();
    for text in &texts[..done] {
        state
            .record_tool_result(id, text, (*text).into())
            .await
            .unwrap();
    }
    turn
}

/// A second runner on the same state, whose only tool is `counter`.
fn restarted(
    state: &Arc<State>,
    counter: &Counter,
    scripts: Vec<Vec<Step>>,
) -> (Arc<FakeProvider>, ThreadRunner, ThreadInput, ThreadOutput) {
    let provider = provider(scripts);
    let config = live_config("resume_config");
    let tool = Box::new(counter.clone());
    let (runner, input, output) = runner_with(state, &provider, &config, model(None), tool);
    (provider, runner, input, output)
}

fn answered(text: &'static str) -> Vec<Vec<Step>> {
    vec![vec![Step::Text(text), Step::Done]]
}

/// Model calls of agent steps; a title request goes without tools and is not one.
fn steps(provider: &FakeProvider) -> usize {
    provider.efforts.lock().unwrap().len()
}

/// The results stored for a call.
async fn results(state: &State, call_id: &str) -> Vec<String> {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .into_iter()
        .filter(|message| message.tool_call_id.as_deref() == Some(call_id))
        .map(|message| message.content)
        .collect()
}

async fn outcome(state: &State, call_id: &str) -> String {
    sqlx::query_scalar("SELECT outcome FROM tool_executions WHERE call_id = ?")
        .bind(call_id)
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

async fn newest_turn(state: &State) -> TurnStatus {
    let status: String =
        sqlx::query_scalar("SELECT status FROM turns ORDER BY started_at DESC, rowid DESC")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    match status.as_str() {
        "running" => TurnStatus::Running,
        "completed" => TurnStatus::Completed,
        "interrupted" => TurnStatus::Interrupted,
        _ => TurnStatus::Failed,
    }
}

fn question(events: &[ThreadEvent]) -> Question {
    events
        .iter()
        .find_map(|event| match event {
            ThreadEvent::Question(question) => Some(question.clone()),
            _ => None,
        })
        .expect("the user is asked")
}

/// Starts the runner, waits for its question, answers with `option` and lets it end.
async fn answer(
    runner: ThreadRunner,
    input: ThreadInput,
    mut output: ThreadOutput,
    option: &str,
) -> Vec<ThreadEvent> {
    let running = tokio::spawn(runner.run());
    let mut events = Vec::new();
    while let Some(event) = output.next_event().await {
        let asked = match &event {
            ThreadEvent::Question(question) => Some(question.reply(option)),
            _ => None,
        };
        events.push(event);
        if let Some(reply) = asked {
            input.send(reply).await.unwrap();
            break;
        }
    }
    drop(input);
    events.extend(drain(&mut output).await);
    running.await.unwrap();
    events
}

/// Lets a runner ask and end with nobody to answer, as when every client went away.
async fn ask_only(
    runner: ThreadRunner,
    input: ThreadInput,
    mut output: ThreadOutput,
) -> Vec<ThreadEvent> {
    drop(input);
    runner.run().await;
    drain(&mut output).await
}
