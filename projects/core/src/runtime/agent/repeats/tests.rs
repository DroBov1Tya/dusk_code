//! The two rules together: which note a repeated call gets, per turn, with the ladder
//! off and on.

use dusk_protocol::thread::RepeatRung;
use serde_json::json;

use super::Repeats;
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "call".into(),
        name: "run_command".into(),
        arguments: json!({ "command": text }),
    }
}

fn failed(content: &str) -> ToolResult {
    ToolResult {
        content: content.into(),
        is_error: true,
        exit_code: Some(1),
        artifacts: Vec::new(),
    }
}

fn ladder_on() -> Limits {
    let mut limits = Limits::default();
    limits.failure_ladder = true;
    limits
}

/// One failing call of `make` with a varied flag; the rung its notice reached, if any.
fn fail(repeats: &mut Repeats, turn: &str, attempt: u32, limits: &Limits) -> Option<RepeatRung> {
    let call = command(&format!("make -j{attempt} V={attempt}"));
    let result = failed("make: *** No rule to make target 'all'.  Stop.");
    let notice = repeats.note(turn, &call, &result, &result.content, limits);
    notice.map(|notice| notice.rung)
}

#[test]
fn the_same_error_climbs_the_ladder_whatever_comes_between() {
    let mut repeats = Repeats::default();
    let limits = ladder_on();
    let read = ToolCall {
        id: "read".into(),
        name: "read_file".into(),
        arguments: json!({ "path": "Makefile" }),
    };
    let mut rungs = Vec::new();
    for attempt in 1..=8 {
        rungs.push(fail(&mut repeats, "t", attempt, &limits));
        // A read between two failures does not start the count over.
        let ok = ToolResult::text("all:\n\tcc main.c".into());
        assert!(
            repeats
                .note("t", &read, &ok, &ok.content, &limits)
                .is_none()
        );
        if attempt < 8 {
            assert!(repeats.take_stop("t").is_none(), "no stop at {attempt}");
        }
    }
    use RepeatRung::{Replan, SameError, Stop};
    assert_eq!(
        rungs,
        [
            None,
            None,
            Some(SameError),
            Some(SameError),
            Some(Replan),
            Some(Replan),
            Some(Replan),
            Some(Stop)
        ]
    );
    let why = repeats.take_stop("t").expect("the turn stops");
    assert!(why.contains("came back 8 times"), "{why}");
    assert!(why.contains("No rule to make target 'all'"), "{why}");
    assert!(why.contains("- make -j1 V=1"), "{why}");
    assert!(repeats.take_stop("t").is_none(), "taken once");
}

#[test]
fn the_notes_list_the_attempts_and_say_what_to_do() {
    let mut repeats = Repeats::default();
    let limits = ladder_on();
    let result = failed("make: *** No rule to make target 'all'.  Stop.");
    let mut notes = Vec::new();
    for attempt in 1..=5 {
        let call = command(&format!("make -j{attempt} V={attempt}"));
        let notice = repeats.note("t", &call, &result, &result.content, &limits);
        notes.push(notice.map(|notice| (notice.count, notice.note)));
    }
    let (count, third) = notes[2].clone().expect("a note at three");
    assert_eq!(count, 3);
    assert!(
        third.contains("- make -j1 V=1\n- make -j2 V=2\n- make -j3 V=3"),
        "{third}"
    );
    assert!(third.contains("Stop varying the command"), "{third}");
    let (_, fifth) = notes[4].clone().expect("a note at five");
    assert!(fifth.contains("the approach is not working"), "{fifth}");
    assert!(fifth.contains("At 8 the turn stops"), "{fifth}");
}

#[test]
fn errors_that_differ_never_climb() {
    let mut repeats = Repeats::default();
    let limits = ladder_on();
    for name in [
        "src", "lib", "app", "web", "docs", "bin", "etc", "tmp", "var", "opt",
    ] {
        let call = command(&format!("ls {name}"));
        let result = failed(&format!("ls: {name}: No such file or directory"));
        assert!(
            repeats
                .note("t", &call, &result, &result.content, &limits)
                .is_none()
        );
    }
    assert!(repeats.take_stop("t").is_none());
}

#[test]
fn with_the_ladder_off_only_the_same_call_is_noted() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let mut rungs = Vec::new();
    for attempt in 1..=10 {
        rungs.push(fail(&mut repeats, "t", attempt, &limits));
    }
    assert!(rungs.iter().all(Option::is_none), "{rungs:?}");
    assert!(repeats.take_stop("t").is_none());
    let call = command("make");
    let result = failed("make: *** No rule to make target 'all'.  Stop.");
    let mut same = Vec::new();
    for _ in 0..3 {
        let notice = repeats.note("t", &call, &result, &result.content, &limits);
        same.push(notice.map(|notice| (notice.rung, notice.count)));
    }
    assert_eq!(same, [None, None, Some((RepeatRung::SameCall, 3))]);
}

#[test]
fn a_new_turn_starts_every_count_over() {
    let mut repeats = Repeats::default();
    let limits = ladder_on();
    for attempt in 1..=8 {
        fail(&mut repeats, "a", attempt, &limits);
    }
    assert!(repeats.take_stop("b").is_none(), "another turn's stop");
    assert_eq!(fail(&mut repeats, "b", 1, &limits), None);
    assert!(repeats.take_stop("a").is_none(), "gone with its turn");
    let call = command("make");
    let ok = ToolResult::text("done".into());
    for _ in 0..2 {
        repeats.note("b", &call, &ok, &ok.content, &limits);
    }
    let same = repeats.note("c", &call, &ok, &ok.content, &limits);
    assert!(
        same.is_none(),
        "the same call counts from one in a new turn"
    );
}
