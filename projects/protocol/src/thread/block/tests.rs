use serde_json::{Value, json};

use super::*;

fn read(arguments: Value) -> Result<Block, String> {
    serde_json::from_value(arguments).map_err(|error| error.to_string())
}

fn text(chars: usize) -> String {
    // Two bytes each: the caps count characters, not bytes.
    "\u{e9}".repeat(chars)
}

fn layers(count: usize) -> Value {
    let layers: Vec<Value> = (0..count)
        .map(|index| json!({ "title": format!("L{index}") }))
        .collect();
    json!({ "flow": { "layers": layers } })
}

fn cards(count: usize) -> Value {
    let cards: Vec<Value> = (0..count)
        .map(|index| json!({ "title": format!("C{index}") }))
        .collect();
    json!({ "flow": { "layers": [{ "title": "a", "cards": cards }, { "title": "b" }] } })
}

fn checks(count: usize) -> Value {
    let items: Vec<Value> = (0..count)
        .map(|index| json!({ "name": format!("c{index}"), "state": "queued" }))
        .collect();
    json!({ "checks": { "items": items } })
}

fn choices(count: usize) -> Value {
    let labels: Vec<String> = (0..count).map(|index| format!("c{index}")).collect();
    json!({ "choices": { "labels": labels } })
}

fn counted(passed: u32, failed: u32, total: u32) -> Value {
    let counts = json!({ "passed": passed, "failed": failed, "total": total });
    json!({ "checks": { "items": [{ "name": "t", "state": "failed", "counts": counts }] } })
}

/// A flow with one text of each kind, each as long as asked.
fn flow_text(title: usize, subtitle: usize, card_title: usize, card_line: usize) -> Value {
    json!({ "flow": { "layers": [
        {
            "title": text(title),
            "subtitle": text(subtitle),
            "cards": [{ "title": text(card_title), "line": text(card_line) }],
        },
        { "title": "b" },
    ] } })
}

#[test]
fn counts_are_accepted_up_to_their_caps_and_refused_past_them() {
    for (arguments, fits) in [
        (layers(MIN_LAYERS - 1), false),
        (layers(MIN_LAYERS), true),
        (layers(MAX_LAYERS), true),
        (layers(MAX_LAYERS + 1), false),
        (cards(0), true),
        (cards(MAX_CARDS), true),
        (cards(MAX_CARDS + 1), false),
        (checks(0), false),
        (checks(1), true),
        (checks(MAX_CHECKS), true),
        (checks(MAX_CHECKS + 1), false),
        (choices(MIN_CHOICES - 1), false),
        (choices(MIN_CHOICES), true),
        (choices(MAX_CHOICES), true),
        (choices(MAX_CHOICES + 1), false),
        (counted(3, 2, 5), true),
        (counted(3, 3, 5), false),
        (counted(u32::MAX, u32::MAX, u32::MAX), false),
        (counted(0, 0, 0), true),
    ] {
        assert_eq!(read(arguments.clone()).is_ok(), fits, "{arguments}");
    }
}

#[test]
fn texts_are_accepted_up_to_their_caps_and_refused_past_them() {
    let (title, subtitle) = (LAYER_TITLE_CHARS, LAYER_SUBTITLE_CHARS);
    let (card, line) = (CARD_TITLE_CHARS, CARD_LINE_CHARS);
    let check = |name: usize, detail: usize| {
        json!({ "checks": { "items": [
            { "name": text(name), "state": "passed", "detail": text(detail) },
        ] } })
    };
    let choice = |chars: usize| json!({ "choices": { "labels": [text(chars), "no"] } });
    for (arguments, fits) in [
        (flow_text(title, subtitle, card, line), true),
        (flow_text(title + 1, subtitle, card, line), false),
        (flow_text(title, subtitle + 1, card, line), false),
        (flow_text(title, subtitle, card + 1, line), false),
        (flow_text(title, subtitle, card, line + 1), false),
        (check(CHECK_NAME_CHARS, CHECK_DETAIL_CHARS), true),
        (check(CHECK_NAME_CHARS + 1, CHECK_DETAIL_CHARS), false),
        (check(CHECK_NAME_CHARS, CHECK_DETAIL_CHARS + 1), false),
        (choice(CHOICE_CHARS), true),
        (choice(CHOICE_CHARS + 1), false),
    ] {
        assert_eq!(read(arguments.clone()).is_ok(), fits, "{arguments}");
    }
}

#[test]
fn a_line_is_trimmed_plain_and_never_empty() {
    assert_eq!(Line::<5>::new("  abc \n").unwrap().as_str(), "abc");
    // The cap is on what is left after the trim.
    assert!(Line::<3>::new(" abc ").is_ok());
    for refused in [
        "",
        "   ",
        "a\nb",
        "a\tb",
        "a\u{0}b",
        "a\u{1b}[31mb",
        "a\u{202e}b",
        "a\u{2028}b",
    ] {
        assert!(Line::<40>::new(refused).is_err(), "{refused:?}");
    }
    let error = Line::<3>::new("abcd").unwrap_err().to_string();
    assert_eq!(error, "the text starting \"abcd\" is over 3 characters");
    // Markup is kept as the characters it is; nothing reads it.
    let kept = "<b>[x](http://y)</b>";
    assert_eq!(Line::<40>::new(kept).unwrap().as_str(), kept);
    assert_eq!(
        serde_json::to_value(Line::<40>::new(kept).unwrap()).unwrap(),
        json!(kept)
    );
}

#[test]
fn unknown_fields_kinds_and_states_are_refused() {
    for arguments in [
        json!({}),
        json!({ "table": { "rows": [] } }),
        json!({ "flow": { "layers": [{ "title": "a" }, { "title": "b" }] }, "x": 1 }),
        json!({ "flow": { "layers": [{ "title": "a" }, { "title": "b" }], "color": "red" } }),
        json!({ "flow": { "layers": [{ "title": "a", "href": "http://x" }, { "title": "b" }] } }),
        json!({ "flow": { "layers": [
            { "title": "a", "cards": [{ "title": "c", "icon": "x" }] },
            { "title": "b" },
        ] } }),
        json!({ "checks": { "items": [{ "name": "a", "state": "passed", "url": "http://x" }] } }),
        json!({ "checks": { "items": [{ "name": "a", "state": "green" }] } }),
        json!({ "checks": { "items": [{ "name": "a", "state": "passed",
            "counts": { "passed": 1, "failed": 0, "total": 1, "flaky": 1 } }] } }),
        json!({ "checks": { "items": [{ "name": "a", "state": "passed",
            "counts": { "passed": -1, "failed": 0, "total": 1 } }] } }),
        json!({ "choices": { "labels": ["a", "b"], "multiple": true } }),
        json!({ "choices": { "labels": ["a", { "label": "b" }] } }),
        json!({ "choices": { "labels": ["a", "b"] }, "checks": checks(1)["checks"] }),
    ] {
        assert!(read(arguments.clone()).is_err(), "{arguments}");
    }
}

#[test]
fn only_one_layer_is_emphasised_and_choices_differ() {
    let flow = |first: bool, second: bool| {
        json!({ "flow": { "layers": [
            { "title": "a", "emphasis": first },
            { "title": "b", "emphasis": second },
        ] } })
    };
    assert!(read(flow(false, true)).is_ok());
    assert_eq!(
        read(flow(true, true)).unwrap_err(),
        "only one layer may have emphasis"
    );
    let repeated = json!({ "choices": { "labels": ["Yes", " Yes "] } });
    assert_eq!(
        read(repeated).unwrap_err(),
        "choice 2 repeats an earlier one"
    );
    let both = json!({ "choices": { "labels": ["a", "b"] }, "checks": checks(1)["checks"] });
    assert_eq!(
        read(both).unwrap_err(),
        "give exactly one of flow, checks and choices"
    );
}

fn flow() -> Value {
    json!({ "flow": { "layers": [
        { "title": "User Intent" },
        {
            "title": "Intelligence Layer",
            "subtitle": "Decides what runs",
            "emphasis": true,
            "cards": [
                { "title": "Router", "line": "Picks the model" },
                { "title": "Memory" },
            ],
        },
        { "title": "Agent Runtime" },
    ] } })
}

fn results() -> Value {
    json!({ "checks": { "items": [
        {
            "name": "cargo test",
            "state": "failed",
            "counts": { "passed": 410, "failed": 2, "total": 412 },
            "detail": "thread::runner::resumes",
        },
        { "name": "clippy", "state": "passed" },
        { "name": "e2e", "state": "running" },
        { "name": "docs", "state": "queued" },
        { "name": "bench", "state": "skipped", "detail": "no baseline" },
    ] } })
}

fn buttons() -> Value {
    json!({ "choices": { "labels": ["Yes, fix it", "Not now"] } })
}

#[test]
fn every_kind_reads_as_plain_text() {
    assert_eq!(
        read(flow()).unwrap().text(),
        "Flow:\n\
         User Intent\n\
         \x20 ↓\n\
         Intelligence Layer (emphasised): Decides what runs\n\
         \x20 - Router: Picks the model\n\
         \x20 - Memory\n\
         \x20 ↓\n\
         Agent Runtime"
    );
    assert_eq!(
        read(results()).unwrap().text(),
        "Checks:\n\
         failed: cargo test (410 passed, 2 failed, 412 total) - thread::runner::resumes\n\
         passed: clippy\n\
         running: e2e\n\
         queued: docs\n\
         skipped: bench - no baseline"
    );
    assert_eq!(
        read(buttons()).unwrap().text(),
        "Choices:\n1. Yes, fix it\n2. Not now"
    );
}

#[test]
fn a_block_survives_a_round_trip_as_the_arguments_it_was_called_with() {
    for arguments in [flow(), results(), buttons()] {
        let block = read(arguments.clone()).unwrap();
        let written = serde_json::to_value(&block).unwrap();
        assert_eq!(written, arguments);
        assert_eq!(read(written).unwrap(), block);
    }
}

#[test]
fn a_block_is_shown_only_by_a_show_call_that_was_not_refused() {
    let call = |name: &str, arguments: Value| ToolCall {
        id: "1".into(),
        name: name.into(),
        arguments,
    };
    let block = read(buttons()).unwrap();
    let shown = call(SHOW_TOOL, buttons());
    assert_eq!(Block::shown(&shown, &block.text()), Some(block.clone()));
    // A plugin's note after the result does not hide the block.
    let noted = format!("{}\n\nnote: seen", block.text());
    assert_eq!(Block::shown(&shown, &noted), Some(block.clone()));
    for refused in [
        "error: tool show is switched off",
        "not run: the user interrupted the turn",
        "Choices: none",
        "",
    ] {
        assert_eq!(Block::shown(&shown, refused), None, "{refused}");
    }
    assert_eq!(Block::shown(&call("bash", buttons()), &block.text()), None);
    assert_eq!(
        Block::shown(&call(SHOW_TOOL, choices(9)), &block.text()),
        None
    );
    let raw = call(SHOW_TOOL, Value::String(buttons().to_string()));
    assert_eq!(Block::shown(&raw, &block.text()), None);
    // The two steps a client takes when the call and its result come apart.
    let asked = Block::asked(&shown).unwrap();
    assert!(asked.shown_by(&block.text()) && !asked.shown_by("error: denied"));
}
