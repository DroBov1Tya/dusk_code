//! `fake=goal-active|goal-paused|goal-stalled|goal-achieved|goal-failed`: a chat working
//! toward a standing goal, built from the events the core sends (the goal, its turns and
//! the evaluator's verdicts), so the indicator, its card and the verdict rows draw as they
//! do live. `open=goal` opens the card. Made-up data only.

use dusk_protocol::{
    model::ToolCall,
    system::now_ms,
    thread::{Goal, GoalEvaluation, GoalStatus, GoalVerdict, ThreadEvent, Turn, TurnStatus},
    tools::ToolResult,
};
use gpui_kit::*;

use crate::{
    pages::chat::transcript::{Item, Transcript},
    workspace::{Center, Workspace},
};

const CONDITION: &str = "All tests in the cache crate pass, including the new expiry tests, \
and `cargo clippy -p cache --all-targets` is clean. Stop after 20 turns and say what is left.";
const MINUTE: i64 = 60_000;

pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let thread_id = ws.shown.clone().unwrap_or_else(|| "shot".to_owned());
    let now = now_ms();
    let goal = |status, turns, tokens, reason: &str, note: Option<&str>| Goal {
        thread_id: thread_id.clone(),
        text: CONDITION.into(),
        status,
        started_at: now - 12 * MINUTE - 4_000,
        turns,
        tokens,
        reason: Some(reason.into()),
        note: note.map(Into::into),
    };
    let verdict = |verdict, turn, tokens, reason: &str| {
        ThreadEvent::GoalEvaluated(GoalEvaluation {
            verdict,
            reason: reason.into(),
            turn,
            tokens,
            elapsed_ms: i64::from(turn) * 4 * MINUTE,
        })
    };
    let not_met = "Three expiry tests fail: `get()` still returns an entry after its deadline. \
                   The agent has not run clippy yet.";
    let mut chat = Transcript::default();
    user(&mut chat, &format!("/goal {CONDITION}"));
    chat.apply(ThreadEvent::Goal {
        goal: Some(goal(GoalStatus::Active, 0, 0, "", None)),
        note: "Goal set".into(),
        started: None,
    });
    user(&mut chat, &format!("Goal: {CONDITION}"));
    turn(
        &mut chat,
        &thread_id,
        "a",
        &[("cargo test -p cache", true)],
        "Three expiry tests fail; `get()` does not look at the deadline yet.",
    );
    chat.apply(verdict(GoalVerdict::NotMet, 1, 18_200, not_met));
    match value {
        "goal-active" | "goal-paused" => {
            user(
                &mut chat,
                &format!("The goal is not met yet: {not_met}\nKeep working toward it."),
            );
            chat.apply(ThreadEvent::TurnStarted(running(&thread_id)));
            chat.apply(ThreadEvent::ToolStarted(ToolCall {
                id: "b1".into(),
                name: "run_command".into(),
                arguments: serde_json::json!({ "command": "cargo test -p cache" }),
            }));
            let paused = value == "goal-paused";
            if paused {
                finish(&mut chat, &thread_id);
            }
            let (status, note) = match paused {
                true => (
                    GoalStatus::Paused,
                    Some("the plan's limit is spent; it goes on after your next message"),
                ),
                false => (GoalStatus::Active, None),
            };
            chat.apply(ThreadEvent::Goal {
                goal: Some(goal(status, 1, 48_200, not_met, note)),
                note: note.map_or_else(String::new, |note| format!("Goal paused: {note}")),
                started: None,
            });
        }
        "goal-stalled" => {
            for turn in 2..=4 {
                let asked =
                    "The goal is not met yet: show the test output.\nKeep working toward it.";
                user(&mut chat, asked);
                answer(
                    &mut chat,
                    &thread_id,
                    "The tests pass now; I fixed the deadline check in the last turn.",
                );
                chat.apply(verdict(
                    GoalVerdict::NotMet,
                    turn,
                    18_200 + u64::from(turn) * 2_100,
                    "The agent says the tests pass but no test run is in the conversation.",
                ));
            }
            chat.apply(ThreadEvent::Goal {
                goal: Some(goal(
                    GoalStatus::Active,
                    4,
                    24_500,
                    "The agent says the tests pass but no test run is in the conversation.",
                    None,
                )),
                note: "Goal stalled: 3 turns in a row used no tool. \
                       It stays set and goes on after your next message."
                    .into(),
                started: None,
            });
        }
        "goal-achieved" => {
            user(
                &mut chat,
                &format!("The goal is not met yet: {not_met}\nKeep working toward it."),
            );
            turn(
                &mut chat,
                &thread_id,
                "b",
                &[
                    ("cargo test -p cache", true),
                    ("cargo clippy -p cache --all-targets", true),
                ],
                "All 17 tests pass and clippy is clean; the output is above.",
            );
            chat.apply(verdict(
                GoalVerdict::Met,
                2,
                41_800,
                "`cargo test -p cache` shows 17 passed, 0 failed, and clippy printed no warning.",
            ));
            chat.apply(ThreadEvent::Goal {
                goal: None,
                note: String::new(),
                started: None,
            });
        }
        "goal-failed" => {
            user(
                &mut chat,
                &format!("The goal is not met yet: {not_met}\nKeep working toward it."),
            );
            turn(
                &mut chat,
                &thread_id,
                "b",
                &[("cargo test -p cache", false)],
                "The expiry tests need a clock the crate cannot fake: `Instant` is not \
                 injectable without changing the public API, which the condition forbids.",
            );
            chat.apply(verdict(
                GoalVerdict::Impossible,
                2,
                39_100,
                "The agent showed that the tests cannot pass without the API change the \
                 condition rules out.",
            ));
            chat.apply(ThreadEvent::Goal {
                goal: None,
                note: String::new(),
                started: None,
            });
        }
        other => eprintln!("no such goal state: {other}"),
    }
    ws.chats.insert(thread_id.clone(), chat);
    ws.shown = Some(thread_id);
    ws.center = Center::Chat;
    cx.notify();
}

fn user(chat: &mut Transcript, text: &str) {
    chat.push(
        Item::User {
            text: text.into(),
            sequence: None,
            answered: Vec::new(),
        },
        now_ms(),
    );
}

fn running(thread_id: &str) -> Turn {
    Turn {
        turn_id: "goal".into(),
        thread_id: thread_id.into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: now_ms() - 40_000,
        finished_at: None,
        error_message: None,
        waiting: None,
        steps: 2,
        tokens: 9_800,
        cost_micro_usd: 0,
    }
}

fn finish(chat: &mut Transcript, thread_id: &str) {
    let mut done = running(thread_id);
    done.status = TurnStatus::Completed;
    done.finished_at = Some(now_ms());
    chat.apply(ThreadEvent::TurnFinished(done));
}

/// A turn that runs `commands` (each passing or not) and ends with `text`.
fn turn(chat: &mut Transcript, thread_id: &str, key: &str, commands: &[(&str, bool)], text: &str) {
    chat.apply(ThreadEvent::TurnStarted(running(thread_id)));
    for (at, (command, passed)) in commands.iter().enumerate() {
        let id = format!("{key}{at}");
        chat.apply(ThreadEvent::ToolStarted(ToolCall {
            id: id.clone(),
            name: "run_command".into(),
            arguments: serde_json::json!({ "command": command }),
        }));
        let mut result = ToolResult::text(if *passed { "ok" } else { "exit 101" }.into());
        result.is_error = !passed;
        chat.apply(ThreadEvent::ToolFinished {
            call_id: id,
            result,
        });
    }
    answer(chat, thread_id, text);
}

/// A turn that only answers, with no tool.
fn answer(chat: &mut Transcript, thread_id: &str, text: &str) {
    if !chat.working {
        chat.apply(ThreadEvent::TurnStarted(running(thread_id)));
    }
    chat.apply(ThreadEvent::Model(dusk_protocol::model::ModelEvent::Text(
        text.into(),
    )));
    finish(chat, thread_id);
}
