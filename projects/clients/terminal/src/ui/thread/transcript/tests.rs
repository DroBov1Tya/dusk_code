use super::thread_lines;
use crate::app::Transcript;
use dusk_protocol::{
    model::ToolCall,
    thread::{
        Block, CARD_LINE_CHARS, CARD_TITLE_CHARS, LAYER_SUBTITLE_CHARS, LAYER_TITLE_CHARS,
        ThreadEvent,
    },
    thread::{MAX_CARDS, MAX_LAYERS},
    tools::ToolResult,
};
use ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};
use serde_json::{Value, json};

/// A thread whose agent called `show` with `arguments`, finished as the core finishes it.
fn shown(arguments: Value) -> Transcript {
    let call = ToolCall {
        id: "a".into(),
        name: "show".into(),
        arguments,
    };
    let content = Block::asked(&call).map_or("error: refused".into(), |block| block.text());
    let mut transcript = Transcript::default();
    transcript.add_user("go");
    transcript.apply(ThreadEvent::ToolStarted(call));
    transcript.apply(ThreadEvent::ToolFinished {
        call_id: "a".into(),
        result: ToolResult {
            is_error: content.starts_with("error:"),
            content,
            exit_code: None,
            artifacts: Vec::new(),
        },
    });
    transcript
}

/// The thread drawn on a test terminal `width` columns wide, row by row.
fn screen(transcript: &Transcript, width: u16) -> Vec<String> {
    let (lines, _) = thread_lines(transcript, width as usize, None, None);
    for line in &lines {
        assert!(line.width() <= width as usize, "{line:?} is over {width}");
    }
    let height = lines.len() as u16;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(lines), frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn text(chars: usize, with: char) -> String {
    std::iter::repeat_n(with, chars).collect()
}

/// Every layer and card at its cap, in characters two columns wide (CJK and an emoji).
fn widest_flow() -> Value {
    let card = json!({ "title": text(CARD_TITLE_CHARS, '\u{6570}'), "line": text(CARD_LINE_CHARS, '\u{636e}') });
    let layer = json!({
        "title": text(LAYER_TITLE_CHARS, '\u{6d41}'),
        "subtitle": text(LAYER_SUBTITLE_CHARS, '\u{1f680}'),
        "cards": vec![card; MAX_CARDS],
    });
    json!({ "flow": { "layers": vec![layer; MAX_LAYERS] } })
}

#[test]
fn a_block_is_drawn_as_the_agents_at_every_width() {
    let checks = json!({ "checks": { "items": [
        { "name": "unit tests", "state": "passed", "counts": { "passed": 214, "failed": 0, "total": 214 } },
        { "name": "lint", "state": "failed", "detail": "2 failed" },
    ] } });
    let choices = json!({ "choices": { "labels": ["Yes", "No"] } });
    for arguments in [widest_flow(), checks, choices] {
        let transcript = shown(arguments);
        assert!(matches!(transcript.entries[1], crate::app::Entry::Block(_)));
        for width in [20, 40, 80] {
            let screen = screen(&transcript, width).join("\n");
            assert!(
                screen.contains("from") && screen.contains("the agent"),
                "{screen}"
            );
            assert!(!screen.contains("show"), "the tool step is gone: {screen}");
        }
    }
    let wide = screen(&shown(widest_flow()), 80).join("\n");
    assert!(
        wide.contains('\u{6570}') && wide.contains('\u{636e}'),
        "{wide}"
    );
    assert!(wide.contains('\u{1f680}'), "{wide}");
}

#[test]
fn a_hostile_block_stays_a_plain_step_and_cannot_steer_the_terminal() {
    for hostile in [
        "ok\x1b[2J\x1b]0;title\x07",
        "admin\u{202e}txt.exe",
        "approve\u{2066}?\u{2069}",
    ] {
        let arguments = json!({ "choices": { "labels": [hostile, "No"] } });
        let transcript = shown(arguments);
        assert!(
            !transcript
                .entries
                .iter()
                .any(|entry| matches!(entry, crate::app::Entry::Block(_)))
        );
        let screen = screen(&transcript, 40).join("\n");
        assert!(
            !screen.contains(['\x1b', '\x07', '\u{202e}', '\u{2066}']),
            "{screen}"
        );
        assert!(!screen.contains("from the agent"), "{screen}");
    }
}

#[test]
fn a_flow_is_boxes_with_arrows_inside_every_width() {
    for width in [60, 80, 120] {
        let screen = screen(&shown(widest_flow()), width);
        let text = screen.join("\n");
        assert!(
            text.contains('\u{256D}') && text.contains('\u{2193}'),
            "{text}"
        );
        assert!(text.contains("from the agent"), "{text}");
        assert!(text.contains('\u{1f680}'), "{text}");
        // Every box edge on a line closes on the same column: no wide character pushed it.
        for line in screen.iter().filter(|line| line.contains('\u{2502}')) {
            assert!(line.trim_end().ends_with('\u{2502}'), "{line}");
        }
        assert_eq!(
            text.matches("\u{2193}").count(),
            MAX_LAYERS - 1,
            "an arrow between layers"
        );
    }
}

#[test]
fn a_flow_at_eighty_columns() {
    let arguments = json!({ "flow": { "layers": [
        { "title": "Request", "subtitle": "what comes in", "cards": [
            { "title": "Parse", "line": "reads the body" }, { "title": "Check" }] },
        { "title": "Handle", "emphasis": true, "cards": [{ "title": "Run", "line": "one" }] },
    ] } });
    let screen = screen(&shown(arguments), 80);
    let text = screen.join("\n");
    assert!(text.contains("\u{2570}"), "{text}");
    assert!(
        text.contains("Request") && text.contains("Handle"),
        "{text}"
    );
    // Cards of one row share a height, so the second box's edge lines up with the first.
    let parse = screen
        .iter()
        .position(|l| l.contains("Parse"))
        .expect("card");
    assert!(
        screen[parse].matches('\u{2502}').count() >= 4,
        "{}",
        screen[parse]
    );
}

#[test]
fn a_narrow_flow_falls_back_to_text() {
    let screen = screen(&shown(widest_flow()), 40).join("\n");
    assert!(!screen.contains('\u{256D}'), "{screen}");
    assert!(screen.contains("from the agent"), "{screen}");
}

#[test]
fn checks_show_every_state_and_the_summary() {
    let arguments = json!({ "checks": { "items": [
        { "name": "unit tests", "state": "passed", "counts": { "passed": 214, "failed": 0, "total": 214 } },
        { "name": "lint", "state": "failed", "counts": { "passed": 3, "failed": 2, "total": 5 }, "detail": "2 failed" },
        { "name": "build", "state": "running" },
        { "name": "deploy", "state": "queued" },
        { "name": "docs", "state": "skipped" },
    ] } });
    for width in [20, 40, 80] {
        let screen = screen(&shown(arguments.clone()), width).join("\n");
        for glyph in ['\u{2713}', '\u{2717}', '\u{25D0}', '\u{25CB}', '\u{2013}'] {
            assert!(screen.contains(glyph), "{glyph} at {width}: {screen}");
        }
    }
    let screen = screen(&shown(arguments), 80).join("\n");
    assert!(
        screen
            .contains("1 passed \u{b7} 1 failed \u{b7} 1 running \u{b7} 1 queued \u{b7} 1 skipped"),
        "{screen}"
    );
    assert!(
        screen.contains("214/214") && screen.contains("2 failed"),
        "{screen}"
    );
}

#[test]
fn checks_at_their_caps_stay_inside_the_width() {
    let item = json!({ "name": text(80, '\u{6570}'), "state": "failed", "detail": text(120, '\u{1f680}'),
        "counts": { "passed": 4_000_000_000_u32, "failed": 1, "total": 4_000_000_001_u32 } });
    let arguments = json!({ "checks": { "items": vec![item; 20] } });
    for width in [20, 40, 80] {
        screen(&shown(arguments.clone()), width);
    }
}

/// A Workshop card is a short list in place of its step, every text on one plain line,
/// with how to add a listing through the Workshop; nothing in it installs.
#[test]
fn a_workshop_card_lists_its_rows_and_says_how_to_add_one() {
    use dusk_protocol::plugin::{Kind, Listing, SUGGEST_TOOL, Suggested, Suggestion};
    let listing = Listing {
        source: "official".into(),
        name: "motion-kit".into(),
        kind: Kind::Pack,
        description: "Springs\x1b[2J and \u{202e}easing".into(),
        skills: vec!["a".into(), "b".into()],
        ..Listing::default()
    };
    let card = Suggestion {
        label: "Animation skills".into(),
        listings: vec![Suggested::of(&listing)],
    };
    let call = ToolCall {
        id: "s".into(),
        name: SUGGEST_TOOL.into(),
        arguments: json!({ "ids": ["official/motion-kit"], "label": "Animation skills" }),
    };
    let mut transcript = Transcript::default();
    transcript.add_user("go");
    transcript.apply(ThreadEvent::ToolStarted(call));
    transcript.apply(ThreadEvent::ToolFinished {
        call_id: "s".into(),
        result: ToolResult::text(card.result()),
    });
    assert!(matches!(
        transcript.entries[1],
        crate::app::Entry::Suggestion(_)
    ));
    let wide = screen(&transcript, 100).join("\n");
    assert!(wide.contains("Animation skills"), "{wide}");
    assert!(wide.contains("motion-kit  pack"), "{wide}");
    assert!(wide.contains("2 skills"), "{wide}");
    assert!(wide.contains("/plugins store <name>"), "{wide}");
    assert!(!wide.contains(['\x1b', '\u{202e}']), "{wide}");
    assert!(
        !wide.contains(SUGGEST_TOOL),
        "the tool step is gone: {wide}"
    );
    // Narrow, every line still fits.
    screen(&transcript, 20);
}

/// A verdict on the goal is one line with its cost; opened, its reason shows under it.
#[test]
fn a_goal_verdict_folds_its_reason() {
    use dusk_protocol::thread::{GoalEvaluation, GoalVerdict};
    let mut transcript = Transcript::default();
    transcript.add_user("/goal the tests pass");
    transcript.apply(ThreadEvent::GoalEvaluated(GoalEvaluation {
        verdict: GoalVerdict::Met,
        reason: "cargo test ran and every test passed".into(),
        turn: 3,
        tokens: 12_400,
        elapsed_ms: 242_000,
    }));
    let folded = screen(&transcript, 80);
    let index = transcript.entries.len() - 1;
    assert!(transcript.is_foldable(index));
    let verdict = folded.iter().find(|row| row.contains("Goal achieved"));
    let verdict = verdict.expect("no verdict line").trim_end().to_owned();
    assert!(
        verdict.ends_with("Goal achieved \u{00B7} 4m 2s \u{00B7} 3 turns \u{00B7} 12.4k tokens"),
        "{verdict}"
    );
    assert!(!folded.iter().any(|row| row.contains("every test passed")));
    transcript.toggle(index);
    let open = screen(&transcript, 80);
    assert!(
        open.iter().any(|row| row.contains("every test passed")),
        "{open:#?}"
    );
    for row in &open {
        println!("{}", row.trim_end());
    }
}
