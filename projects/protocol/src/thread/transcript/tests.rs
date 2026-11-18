use super::*;
use crate::{model::ToolCall, thread::ThreadEvent, tools::ToolResult};

fn call(arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "1".into(),
        name: "bash".into(),
        arguments,
    }
}

#[test]
fn a_step_shows_the_first_line_of_what_it_was_asked() {
    let tool = Tool::new(
        &call(serde_json::json!({ "command": "ls\nmore" })),
        ToolState::Running,
    );
    assert_eq!(tool.detail, "ls");
    let tool = Tool::new(&call(serde_json::json!({ "x": 1 })), ToolState::Done);
    assert_eq!(tool.detail, "");
    let long = "y".repeat(500);
    assert_eq!(tool_detail(&serde_json::json!({ "path": long })).len(), 160);
}

#[test]
fn a_result_lands_on_its_step_and_long_text_is_clipped() {
    let mut tool = Tool::new(&call(serde_json::json!({})), ToolState::Running);
    tool.finish(&ToolResult::text("out".into()));
    assert_eq!((tool.state, tool.output.as_str()), (ToolState::Done, "out"));
    let mut failed = ToolResult::text("boom".into());
    failed.is_error = true;
    tool.finish(&failed);
    assert_eq!(tool.state, ToolState::Failed);
    let long = "\u{e9}".repeat(SHOWN_CHARS + 1);
    let clipped = clip(&long);
    assert!(clipped.ends_with("\n..."));
    assert_eq!(clipped.chars().count(), SHOWN_CHARS + 4);
    assert_eq!(clip("short"), "short");
}

#[test]
fn events_leave_their_lines_and_messages_leave_none() {
    assert_eq!(
        note(&ThreadEvent::Failed("x".into())),
        Some(Note::Error("x".into()))
    );
    assert_eq!(
        note(&ThreadEvent::ArbiterDecided {
            what: "rm".into(),
            allow: false,
            reason: "no".into()
        }),
        Some(Note::Notice("Arbiter denied rm: no".into()))
    );
    assert_eq!(note(&ThreadEvent::Idle), None);
    let stubbed = ThreadEvent::ResultsStubbed {
        results: 3,
        saved_tokens: 9000,
    };
    assert_eq!(note(&stubbed), None, "a move leaves no row in the chat");
    let shaped = ThreadEvent::ResultShaped { saved_tokens: 900 };
    assert_eq!(
        note(&shaped),
        None,
        "a shaped result leaves no row in the chat"
    );
    let recorded = ThreadEvent::EpisodeRecorded {
        kind: crate::thread::EpisodeKind::Fixed,
    };
    assert_eq!(
        note(&recorded),
        None,
        "an episode leaves no row in the chat"
    );
}

fn retrying(attempt: u32, max_attempts: Option<u32>) -> ThreadEvent {
    ThreadEvent::Retrying {
        attempt,
        error: "could not reach api.example.com".into(),
        kind: crate::model::ProviderErrorKind::Unreachable,
        wait_ms: 2000,
        max_attempts,
    }
}

/// Five failed attempts of one call leave one row that the answer takes away, and one
/// line in a chat that does not keep the row.
#[test]
fn a_call_tried_again_keeps_one_row_until_it_answers() {
    let mut row = None;
    let mut lines = Vec::new();
    let events = (1..=5)
        .map(|attempt| retrying(attempt, Some(6)))
        .chain([ThreadEvent::Model(crate::model::ModelEvent::Text(
            "hi".into(),
        ))]);
    for event in events {
        match retry_row(&event) {
            Some(RetryRow::Show(retry)) => row = Some(retry),
            Some(RetryRow::Clear) => row = None,
            None => {}
        }
        lines.extend(note(&event));
        if let ThreadEvent::Retrying { attempt: 5, .. } = event {
            let shown = row.as_ref().unwrap();
            assert_eq!(shown.attempt, 5);
            assert_eq!(shown.next_attempt(), "attempt 6 of 6");
        }
    }
    assert_eq!(row, None);
    assert_eq!(
        lines,
        [Note::Notice(
            "Provider unreachable: could not reach api.example.com; trying again, attempt 2 of 6"
                .into()
        )]
    );
    let budget = retry_row(&retrying(8, None));
    assert!(matches!(budget, Some(RetryRow::Show(retry)) if retry.next_attempt() == "attempt 9"));
}

#[test]
fn a_failed_turn_leaves_one_error_that_names_its_kind_and_attempts() {
    use crate::model::{ProviderErrorKind, ProviderFailure};
    use crate::thread::TurnFailure;
    let failure = |provider| {
        ThreadEvent::TurnFailed(TurnFailure {
            error: "could not reach api.example.com".into(),
            provider,
        })
    };
    let unreachable = failure(Some(ProviderFailure {
        kind: ProviderErrorKind::Unreachable,
        attempts: 9,
    }));
    assert_eq!(
        note(&unreachable),
        Some(Note::Error(
            "Provider unreachable after 9 attempts: could not reach api.example.com".into()
        ))
    );
    assert_eq!(retry_row(&unreachable), Some(RetryRow::Clear));
    let once = failure(Some(ProviderFailure {
        kind: ProviderErrorKind::Auth,
        attempts: 1,
    }));
    assert_eq!(
        note(&once),
        Some(Note::Error(
            "Key or sign-in not accepted: could not reach api.example.com".into()
        ))
    );
    assert_eq!(
        note(&failure(None)),
        Some(Note::Error("could not reach api.example.com".into()))
    );
}

/// A turn that starts waiting takes the retry row away and leaves one line in its place.
#[test]
fn a_waiting_turn_leaves_one_line_and_no_retry_row() {
    use crate::model::ProviderErrorKind;
    use crate::thread::{Turn, TurnStatus, TurnWait};
    let waiting = ThreadEvent::TurnWaiting(Turn {
        turn_id: "turn".into(),
        thread_id: "thread".into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: 0,
        finished_at: None,
        error_message: None,
        waiting: TurnWait::provider(ProviderErrorKind::Quota, 0, 60_000),
        steps: 0,
        tokens: 0,
        cost_micro_usd: 0,
    });
    assert_eq!(retry_row(&waiting), Some(RetryRow::Clear));
    assert_eq!(
        note(&waiting),
        Some(Note::Notice("Quota used up; continues by itself".into()))
    );
}
