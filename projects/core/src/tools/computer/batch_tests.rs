//! A batch is checked and asked for whole before any step runs. No test here touches
//! the desktop: steps that would are refused first, or replaced by fakes.

use std::sync::Arc;

use serde_json::json;
use tokio::sync::mpsc;

use super::*;
use crate::{
    config::ApprovalPolicy,
    runtime::ThreadEvent,
    state::artifacts::ArtifactStore,
    testing::{temp_dir, tool_context},
    tools::{
        Approvals, Decision, Judge, Request, ToolScope, Verdict,
        computer::screen::View,
        tool::{Progress, ToolContext},
    },
};

fn batch() -> ComputerActions {
    ComputerActions(Arc::new(Screen::new(ArtifactStore::new(
        std::env::temp_dir(),
    ))))
}

/// A context that asks through `approvals`; with no `progress` no one can answer.
fn context(approvals: &Arc<Approvals>, progress: Option<Progress>) -> ToolContext {
    let dir = temp_dir("computer_batch");
    std::fs::create_dir_all(&dir).unwrap();
    let mut context = tool_context(&ToolScope::new(&dir, &dir).unwrap());
    context.approvals = Some(Arc::clone(approvals));
    context.progress = progress;
    context
}

fn error(result: Result<ToolResult>) -> String {
    format!("{:#}", result.expect_err("the batch should fail"))
}

fn fake() -> Request {
    Request::Screen {
        action: "fake".into(),
    }
}

#[test]
fn too_many_steps_and_unknown_tools_are_refused() {
    let step = json!({"tool": "press_keys", "arguments": {"keys": "enter"}});
    let steps = vec![step; MAX_STEPS + 1];
    assert!(batch().steps(json!({ "steps": steps })).is_err());
    for wrong in [
        json!({"steps": []}),
        json!({"steps": [{"tool": "run_command", "arguments": {}}]}),
        json!({"steps": [{"tool": "window_session", "arguments": {}}]}),
        json!({"steps": [{"tool": "click", "arguments": {}, "extra": 1}]}),
        json!({"steps": [], "extra": 1}),
    ] {
        assert!(batch().steps(wrong.clone()).is_err(), "{wrong}");
    }
}

#[tokio::test]
async fn a_wrong_step_stops_the_batch_before_anything_is_asked() {
    let approvals = Arc::new(Approvals::default());
    // A step that reached approval would fail with "no one can answer here" instead.
    let arguments = json!({"steps": [
        {"tool": "press_keys", "arguments": {"keys": "enter"}},
        {"tool": "type_text", "arguments": {"text": ""}},
    ]});
    let message = error(
        batch()
            .call_result(arguments, context(&approvals, None))
            .await,
    );
    assert!(message.contains("step 2 (type_text)"), "{message}");
    assert!(message.contains("no step ran"), "{message}");
    assert!(!message.contains("approval"), "{message}");
}

#[tokio::test]
async fn every_step_reads_the_picture_seen_before_the_batch() {
    let screen = Arc::new(Screen::new(ArtifactStore::new(std::env::temp_dir())));
    screen.set_coordinates(Some(View::scaled(1.0)));
    let shot = Arc::clone(&screen);
    let reader = Arc::clone(&screen);
    let steps = vec![
        (
            StepTool::Screenshot,
            Step::new(fake(), move || {
                shot.set_coordinates(Some(View::scaled(2.0)));
                Ok(ToolResult::text("shot".into()))
            }),
        ),
        (
            StepTool::Click,
            Step::new(fake(), move || {
                let at = reader.point(10.0, 10.0)?;
                Ok(ToolResult::text(format!("at {}", at.x)))
            }),
        ),
    ];
    let result = run(&screen, steps).await;
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains("2. at 10"), "{}", result.content);
    // After the batch the newest picture, the one the model now sees, counts again.
    assert_eq!(screen.point(10.0, 10.0).unwrap().x, 20.0);
}

#[tokio::test]
async fn a_failed_step_stops_the_rest() {
    let screen = Screen::new(ArtifactStore::new(std::env::temp_dir()));
    let ran = Arc::new(parking_lot::Mutex::new(0));
    let counter = Arc::clone(&ran);
    let steps = vec![
        (
            StepTool::Click,
            Step::new(fake(), || anyhow::bail!("no control there")),
        ),
        (
            StepTool::Click,
            Step::new(fake(), move || {
                *counter.lock() += 1;
                Ok(ToolResult::text(String::new()))
            }),
        ),
    ];
    let result = run(&screen, steps).await;
    assert!(result.is_error);
    assert!(result.content.contains("1. failed: no control there"));
    assert!(result.content.contains("steps 2 to 2 did not run"));
    assert_eq!(*ran.lock(), 0);
}

/// Allows the screen for the session, as a user would.
async fn allow_screen(approvals: &Approvals) {
    let (events, mut seen) = mpsc::unbounded_channel();
    let progress = Progress {
        call_id: "grant".into(),
        events,
    };
    let ask = approvals.ask(fake(), Some(progress), ApprovalPolicy::Ask);
    let answer = async {
        let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
            panic!("missing request");
        };
        assert!(approvals.answer(asked.id, Decision::Session));
    };
    let (result, ()) = tokio::join!(ask, answer);
    result.unwrap();
}

#[tokio::test]
async fn one_refused_step_refuses_the_batch() {
    let approvals = Arc::new(Approvals::default());
    allow_screen(&approvals).await;
    let arguments = json!({"steps": [
        {"tool": "screenshot", "arguments": {}},
        {"tool": "press_keys", "arguments": {"keys": "enter"}},
    ]});
    // The screenshot alone would pass on the grant; the keys still need a yes, and no
    // one can answer here, so nothing runs.
    let message = error(
        batch()
            .call_result(arguments, context(&approvals, None))
            .await,
    );
    assert!(message.contains("press enter"), "{message}");

    // Asked, the batch lists every step by its number, the allowed ones marked, so
    // the click that sets the focus is seen with the typing; a no stops all.
    let (events, mut seen) = mpsc::unbounded_channel();
    let progress = Progress {
        call_id: "batch".into(),
        events,
    };
    let arguments = json!({"steps": [
        {"tool": "screenshot", "arguments": {}},
        {"tool": "type_text", "arguments": {"text": "hi"}},
        {"tool": "press_keys", "arguments": {"keys": "Shift + Enter"}},
    ]});
    let tool = batch();
    let call = tool.call_result(arguments, context(&approvals, Some(progress)));
    let answer = async {
        let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
            panic!("missing request");
        };
        assert!(matches!(asked.request, Request::Keyboard { .. }));
        let lines: Vec<String> = asked.steps.iter().map(|step| step.line()).collect();
        assert_eq!(
            lines,
            [
                "1. using the screen: take a screenshot (already allowed)",
                "2. using the keyboard: type \"hi\"",
                "3. using the keyboard: press shift+enter",
            ]
        );
        assert_eq!(
            asked.title("the agent"),
            "the agent wants to run 3 computer actions"
        );
        // A surface that reads only the request's text, or its plain text, sees every
        // step too; the row drawing clients use the title, so nothing shows twice.
        let described = asked.request.describe("the agent");
        let text = asked.text("the agent");
        for line in &lines {
            assert!(described.contains(line.as_str()), "{described}");
            assert!(text.contains(line.as_str()), "{text}");
        }
        assert!(!asked.title("the agent").contains("1."));
        assert_eq!(asked.session, None);
        assert!(approvals.answer(asked.id, Decision::Deny));
    };
    let (result, ()) = tokio::join!(call, answer);
    assert!(error(result).contains("denied"));
}

/// Says no to any request that presses keys, yes to the rest.
struct NoKeys;

impl Judge for NoKeys {
    fn judge<'a>(&'a self, request: &'a Request) -> crate::BoxFuture<'a, Verdict> {
        let allow = !request.what().contains("press");
        Box::pin(async move {
            Ok(Verdict {
                allow,
                reason: "keys are not allowed".into(),
            })
        })
    }
}

#[tokio::test]
async fn the_arbiter_judges_every_step_alone() {
    let approvals = Approvals::default();
    approvals.set_judge(Arc::new(NoKeys));
    let requests = vec![
        fake(),
        Request::Keyboard {
            action: "press enter".into(),
        },
    ];
    let denied = approvals
        .ask_all(requests, None, ApprovalPolicy::Arbiter)
        .await
        .expect_err("one step is denied");
    let message = format!("{denied:#}");
    assert!(message.starts_with("step 2 of the batch"), "{message}");
    assert!(message.contains("keys are not allowed"), "{message}");
    approvals
        .ask_all(vec![fake(), fake()], None, ApprovalPolicy::Arbiter)
        .await
        .unwrap();
}

#[tokio::test]
async fn mixed_kinds_offer_no_session_grant() {
    let approvals = Approvals::default();
    let requests = vec![
        fake(),
        Request::SystemPointer {
            action: "scroll".into(),
        },
    ];
    let (events, mut seen) = mpsc::unbounded_channel();
    let progress = Progress {
        call_id: "mixed".into(),
        events,
    };
    let ask = approvals.ask_all(requests.clone(), Some(progress), ApprovalPolicy::Ask);
    let answer = async {
        let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
            panic!("missing request");
        };
        assert!(matches!(asked.request, Request::SystemPointer { .. }));
        assert_eq!(asked.session, None);
        // A client that answers "session" anyway grants nothing beyond this time.
        assert!(approvals.answer(asked.id, Decision::Session));
    };
    let (result, ()) = tokio::join!(ask, answer);
    result.unwrap();
    for request in requests {
        assert!(
            approvals
                .ask(request, None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
    }
}
