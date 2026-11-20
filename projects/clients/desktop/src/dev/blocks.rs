//! `fake=blocks`: a chat with a Flow, a Checks and reply buttons in it, and the same chat
//! with a turn at work or waiting, to look at the blocks and the working row without a model.
//! Made-up data only.

use dusk_protocol::{
    model::ProviderErrorKind,
    system::now_ms,
    thread::{Block, ThreadEvent, Turn, TurnStatus, TurnWait},
};
use gpui_kit::*;

use crate::{
    pages::chat::transcript::{Item, Transcript},
    workspace::{Center, Workspace},
};

/// `blocks` is the chat at rest; `blocks-working`, `blocks-wait` and `blocks-decision` add a
/// turn under way, waiting for the provider, and waiting for the user.
pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let block = |json: serde_json::Value| -> Item {
        Item::Block(serde_json::from_value::<Block>(json).expect("a made-up block is within caps"))
    };
    let said = |text: &str| Item::Assistant {
        text: text.into(),
        reasoning: String::new(),
    };
    let mut chat = Transcript::default();
    chat.extend(vec![
        Item::User {
            text: "How does a request travel through the importer, and is it safe to ship?".into(),
            sequence: Some(1),
            answered: Vec::new(),
        },
        said("The importer has three layers."),
        block(serde_json::json!({ "flow": { "layers": [
            { "title": "Intake", "subtitle": "Where a file enters and is checked before anything is read",
              "cards": [
                { "title": "Upload", "line": "size cap 8 MiB, type by signature" },
                { "title": "Watch folder", "line": "inside the project only" },
                { "title": "API", "line": "one row a request, rate limited" } ] },
            { "title": "Mapping", "subtitle": "Rows become typed records; this is where the failing test is",
              "emphasis": true,
              "cards": [
                { "title": "Parser", "line": "CSV and JSON lines" },
                { "title": "RowKind", "line": "the new enum, unknown kinds refused" },
                { "title": "Validator", "line": "strict schema, no extra fields" },
                { "title": "Dedupe", "line": "by content hash" } ] },
            { "title": "Storage",
              "cards": [
                { "title": "Writer", "line": "one transaction a batch" },
                { "title": "Audit log", "line": "who imported what, never the rows" } ] },
        ] } })),
        block(serde_json::json!({ "checks": { "items": [
            { "name": "parser unit tests", "state": "passed",
              "counts": { "passed": 214, "failed": 0, "total": 214 } },
            { "name": "clippy", "state": "passed" },
            { "name": "writer integration tests", "state": "failed",
              "counts": { "passed": 11, "failed": 1, "total": 12 },
              "detail": "a_batch_rolls_back_on_a_bad_row: left 3, right 0" },
            { "name": "mapper property tests", "state": "running" },
            { "name": "fuzz the parser", "state": "queued" },
            { "name": "benchmarks", "state": "skipped" },
        ] } })),
        said(
            "Not yet safe to ship: the writer does not roll a batch back. Shall I fix that first?",
        ),
        block(serde_json::json!({ "choices": {
            "labels": ["Fix the writer", "Show the test", "Not now"] } })),
    ], 0);
    let turn = |waiting| Turn {
        turn_id: "turn".into(),
        thread_id: "shot".into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: now_ms() - 134_000,
        finished_at: None,
        error_message: None,
        waiting,
        steps: 0,
        tokens: 0,
        cost_micro_usd: 0,
    };
    let wait = |event: fn(Turn) -> ThreadEvent, waiting| {
        let mut turn = turn(waiting);
        turn.started_at = now_ms() - 134_000;
        event(turn)
    };
    match value {
        "blocks-working" => {
            chat.apply(wait(ThreadEvent::TurnStarted, None));
            for _ in 0..6 {
                chat.apply(ThreadEvent::Model(
                    dusk_protocol::model::ModelEvent::Finished(
                        dusk_protocol::model::FinishReason::ToolCalls,
                    ),
                ));
            }
        }
        "blocks-wait" => {
            chat.apply(wait(ThreadEvent::TurnStarted, None));
            let wake = now_ms() + 12 * 60_000 + 5_000;
            let waiting = TurnWait::provider(ProviderErrorKind::RateLimited, now_ms(), wake);
            chat.apply(wait(ThreadEvent::TurnWaiting, waiting));
        }
        "blocks-decision" => {
            chat.apply(wait(ThreadEvent::TurnStarted, None));
            let waiting = Some(TurnWait::Decision { since: now_ms() });
            chat.apply(wait(ThreadEvent::TurnWaiting, waiting));
        }
        // A light question takes the place of the block's buttons, in the dock.
        "blocks-question" => {
            chat.items.pop();
            chat.apply(ThreadEvent::Question(dusk_protocol::tools::Question {
                question: "Which should I fix first?".into(),
                options: vec!["The writer".into(), "The mapper".into(), "Neither".into()],
                multiple: false,
            }));
        }
        _ => {}
    }
    ws.chats.insert("shot".into(), chat);
    ws.shown = Some("shot".into());
    ws.center = Center::Chat;
    cx.notify();
}
