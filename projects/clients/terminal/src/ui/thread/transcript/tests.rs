use super::thread_lines;
use crate::app::Transcript;
use dusk_protocol::{
    model::ToolCall,
    thread::{Block, CARD_LINE_CHARS, CARD_TITLE_CHARS, LAYER_SUBTITLE_CHARS, LAYER_TITLE_CHARS},
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
    transcript.start_tool(call);
    transcript.finish_tool(
        "a",
        ToolResult {
            is_error: content.starts_with("error:"),
            content,
            exit_code: None,
            artifacts: Vec::new(),
        },
    );
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
