//! The judge's answer is untrusted model text: it is read when it is a JSON object
//! anywhere in the text, brought into range, and anything else lets the turn go on. A
//! stall candidate is given once per mark, waits longer after each judge, and stops
//! coming once the judge's calls are spent.

use dusk_protocol::thread::RepeatRung;
use serde_json::json;

use super::{Digest, Facts, Repeats, Stall, digest, judge_stop, verdict};
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

fn stall() -> Stall {
    Stall {
        count: 30,
        what: "30 steps in a row brought nothing new".into(),
    }
}

#[test]
fn a_verdict_is_read_from_json_anywhere_in_the_answer() {
    let found = verdict(
        "Sure.\n```json\n{\"stuck\": true, \"confidence\": 0.95, \"redirect\": \"Read the \
         error.\"}\n```",
    )
    .unwrap();
    assert!(found.stuck && found.sure_stuck());
    assert_eq!(found.redirect, "Read the error.");
    let unsure = verdict(r#"{"stuck": true, "confidence": 0.6, "redirect": "x"}"#).unwrap();
    assert!(!unsure.sure_stuck(), "only a confident judge ends a turn");
    let words = verdict(r#"{"stuck": "yes", "confidence": "0.91"}"#).unwrap();
    assert!(words.sure_stuck());
    assert_eq!(words.redirect, "");
}

#[test]
fn an_answer_that_cannot_be_read_is_no_verdict() {
    for text in [
        "",
        "STUCK",
        "{not json}",
        "} {",
        r#"{"confidence": 1.0}"#,
        r#"{"stuck": "maybe", "confidence": 1.0}"#,
        r#"{"stuck": 1, "confidence": 1.0}"#,
        "{\"stuck\": true, \"confidence\": 1.0",
    ] {
        assert_eq!(verdict(text), None, "{text:?}");
    }
}

#[test]
fn confidence_and_redirect_are_bounded() {
    let high = verdict(r#"{"stuck": true, "confidence": 7}"#).unwrap();
    assert_eq!(high.confidence, 1.0);
    let low = verdict(r#"{"stuck": true, "confidence": -3}"#).unwrap();
    assert_eq!(low.confidence, 0.0);
    let text = json!({ "stuck": false, "confidence": 0.2, "redirect":
        format!("line one\nline\u{202e}two\u{0007} {}", "x".repeat(5000)) })
    .to_string();
    let long = verdict(&text).unwrap();
    assert!(!long.redirect.contains('\n') && !long.redirect.contains('\u{202e}'));
    assert!(!long.redirect.contains('\u{0007}'));
    assert!(
        long.redirect.starts_with("line one linetwo "),
        "{}",
        long.redirect
    );
    assert!(long.redirect.chars().count() <= 310);
}

#[test]
fn the_digest_and_the_stop_say_what_was_tried() {
    let kept = Digest {
        calls: (0..12)
            .map(|i| format!("cargo test case_{i} -> failed, exit 101"))
            .collect(),
        error: Some("this error came back 4 times:\nerror: boom".into()),
    };
    let text = digest(&"fix the parser ".repeat(500), &stall(), &kept);
    assert!(
        text.starts_with("What the rule saw: 30 steps in a row"),
        "{text}"
    );
    assert!(text.contains("case_0 -> failed") && text.contains("case_11"));
    assert!(text.contains("error: boom"));
    assert!(text.len() < 4000, "the request is clipped: {}", text.len());
    let found = verdict(r#"{"stuck": true, "confidence": 0.97, "redirect": "Ask the user."}"#);
    let why = judge_stop(&stall(), &found.unwrap(), &kept);
    assert!(why.starts_with("the agent stopped: 30 steps"), "{why}");
    assert!(!why.contains("case_3 ") && why.contains("- cargo test case_4"));
    assert!(why.ends_with("suggested: Ask the user."), "{why}");
}

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "call".into(),
        name: "run_command".into(),
        arguments: json!({ "command": text }),
    }
}

/// Runs `text` as its own step with the same result each time; the notice's rung.
fn step(repeats: &mut Repeats, text: &str, limits: &Limits) -> Option<RepeatRung> {
    repeats.step("t");
    let result = ToolResult {
        content: "same".into(),
        is_error: false,
        exit_code: Some(0),
        artifacts: Vec::new(),
    };
    let facts = Facts {
        read_only: true,
        ..Facts::default()
    };
    let call = command(text);
    repeats
        .note("t", &call, &result, "same", &facts, limits)
        .map(|notice| notice.rung)
}

#[test]
fn a_cycle_asked_twice_to_replan_is_a_stall_and_judges_wait_longer() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let mut replans = 0;
    let mut stalls = Vec::new();
    for i in 0..40 {
        // A two-call cycle: status, diff, status, diff...
        let text = if i % 2 == 0 { "git status" } else { "git diff" };
        if step(&mut repeats, text, &limits) == Some(RepeatRung::CycleReplan) {
            replans += 1;
        }
        if let Some(stall) = repeats.take_stall("t", &limits) {
            stalls.push(replans);
            assert!(stall.what.contains("came round"), "{}", stall.what);
            repeats.judged("t");
        }
    }
    // The 2nd re-plan marks one; after a judge one mark passes, after the next two.
    assert_eq!(stalls, [2, 6, 12]);
    let digest = repeats.digest("t");
    assert_eq!(digest.calls.len(), super::judge::RECENT_CALLS);
    assert_eq!(digest.calls.last().unwrap(), "git diff -> ok");
    // The default three calls are spent.
    for i in 0..40 {
        let text = if i % 2 == 0 { "git status" } else { "git diff" };
        step(&mut repeats, text, &limits);
        assert_eq!(repeats.take_stall("t", &limits), None);
    }
}

#[test]
fn with_the_judge_off_no_stall_is_given() {
    let mut repeats = Repeats::default();
    let mut limits = Limits::default();
    limits.loop_judge = false;
    for i in 0..40 {
        let text = if i % 2 == 0 { "git status" } else { "git diff" };
        step(&mut repeats, text, &limits);
        assert_eq!(repeats.take_stall("t", &limits), None);
    }
    assert!(repeats.digest("t").calls.is_empty());
}

#[test]
fn varied_calls_make_no_stall() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for i in 0..60 {
        let result = ToolResult {
            content: format!("file {i}"),
            is_error: false,
            exit_code: None,
            artifacts: Vec::new(),
        };
        repeats.step("t");
        let call = ToolCall {
            id: "c".into(),
            name: "read_file".into(),
            arguments: json!({ "path": format!("src/f{i}.rs") }),
        };
        let facts = Facts {
            read_only: true,
            ..Facts::default()
        };
        assert!(
            repeats
                .note("t", &call, &result, &result.content, &facts, &limits)
                .is_none()
        );
        assert_eq!(repeats.take_stall("t", &limits), None);
    }
}
