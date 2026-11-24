//! Blocks the agent showed with `show` or `draw_diagram`. Every text in one is a protocol
//! `Line`: drawn as it is, never as markdown or a link. Each block says it is the agent's,
//! so it cannot pass for a prompt of the app.

use dusk_protocol::thread::{Block, CheckState, Checks, Counts, Flow, FlowCard, Layer};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::blocks::{indent, verbatim};
use crate::ui::text::{
    boxes::{self, FRAME},
    format::printable,
};
use crate::ui::thread::{
    theme::{
        ACCENT, DONE, DOT, FAILED, GUTTER, HAIRLINE, NEGATIVE, OFF, POSITIVE, TEXT, TEXT_SOFT,
        TOOL, bold, dim, fg,
    },
    wrap,
};

/// Under this many columns a flow has no room for two cards side by side, so it is shown
/// as text. It is the design's 60 columns of terminal less what the thread keeps for
/// its margins.
const MIN_FLOW_WIDTH: usize = 56;

/// The widest a layer's box grows; past it lines are too long to follow.
const MAX_BOX_WIDTH: usize = 100;

/// A card's columns, its edges included, and the space between cards.
const CARD_WIDTH: usize = 24;
const CARD_GAP: usize = 1;

/// A card's line is cut after this many rows.
const CARD_LINE_ROWS: usize = 2;

/// What the thread keeps free on the left of a block.
const MARGIN: &str = "   ";

/// The glyph of a check that runs: a half ring, which stands still.
const RUNNING: &str = "\u{25D0}";
const SKIPPED: &str = "\u{2013}";

/// A block as lines `width` wide, past the entries' left margin.
pub(super) fn render(block: &Block, width: usize) -> Vec<Line<'static>> {
    match block {
        Block::Flow(flow) if width >= MIN_FLOW_WIDTH => draw_flow(flow, width),
        Block::Flow(_) => as_text(block, "flow", width),
        Block::Checks(checks) => draw_checks(checks, width),
        Block::Choices(_) => as_text(block, "choices", width),
        Block::Diagram(diagram) => super::diagram::render(diagram, width),
    }
}

/// The block's plain text, `Block::text()`, under a head that names its kind and says
/// whose it is: the form for a kind without a drawing of its own, or too narrow for one.
pub(super) fn as_text(block: &Block, kind: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = head(kind, width);
    let text = block.text();
    // The first line is the kind's heading, which the head says already. The protocol
    // keeps control characters out of a block; the terminal does not rely on that.
    let body: Vec<String> = text.lines().skip(1).map(printable).collect();
    let body = verbatim(&body.join("\n"), width.saturating_sub(2), Style::new());
    lines.extend(indent(body, &format!("   {GUTTER} ")));
    lines
}

/// The kind and "from the agent", wrapped to the width; the words stay whole.
pub(super) fn head(kind: &str, width: usize) -> Vec<Line<'static>> {
    let words = vec![
        Span::styled(kind.to_owned(), fg(TEXT_SOFT)),
        Span::styled(format!(" {DOT} from the agent"), dim()),
    ];
    wrap::words(words, width.saturating_sub(2), 0)
        .into_iter()
        .enumerate()
        .map(|(row, spans)| {
            let lead = if row == 0 {
                format!("   {TOOL} ")
            } else {
                "     ".into()
            };
            Line::from([vec![Span::styled(lead, dim())], spans].concat())
        })
        .collect()
}

/// Layers as boxes, one under the other with an arrow between; the emphasised one in
/// the accent.
fn draw_flow(flow: &Flow, width: usize) -> Vec<Line<'static>> {
    let box_width = width.min(MAX_BOX_WIDTH);
    let mut lines = head("flow", width);
    for (at, layer) in flow.layers().iter().enumerate() {
        if at > 0 {
            let pad = " ".repeat(MARGIN.len() + box_width / 2);
            lines.push(Line::from(Span::styled(format!("{pad}\u{2193}"), dim())));
        }
        lines.extend(layer_box(layer, box_width).into_iter().map(margin));
    }
    lines
}

fn margin(line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::raw(MARGIN)];
    spans.extend(line.spans);
    Line::from(spans)
}

fn layer_box(layer: &Layer, width: usize) -> Vec<Line<'static>> {
    let (border, title) = match layer.emphasis {
        true => (fg(ACCENT), bold(ACCENT)),
        false => (fg(HAIRLINE), bold(TEXT)),
    };
    let inner = width.saturating_sub(FRAME);
    let mut lines = vec![boxes::top(layer.title.as_str(), width, border, title)];
    if let Some(subtitle) = &layer.subtitle {
        let subtitle = Span::styled(printable(subtitle.as_str()), dim());
        lines.push(boxes::row(vec![subtitle], width, border));
    }
    let across = boxes::columns(inner, CARD_WIDTH, CARD_GAP);
    for cards in layer.cards().chunks(across) {
        let drawn: Vec<Vec<Line<'static>>> = {
            let height = cards.iter().map(card_height).max().unwrap_or(0);
            cards.iter().map(|card| card_box(card, height)).collect()
        };
        for row in 0..drawn.first().map_or(0, Vec::len) {
            let mut spans = Vec::new();
            for (at, card) in drawn.iter().enumerate() {
                if at > 0 {
                    spans.push(Span::raw(" ".repeat(CARD_GAP)));
                }
                spans.extend(card[row].spans.clone());
            }
            lines.push(boxes::row(spans, width, border));
        }
    }
    lines.push(boxes::bottom(width, border));
    lines
}

/// The rows of a card's line, cut to what a card shows.
fn card_line_rows(card: &FlowCard) -> Vec<String> {
    let Some(line) = &card.line else {
        return Vec::new();
    };
    let words = vec![Span::raw(printable(line.as_str()))];
    let mut rows: Vec<String> = wrap::words(words, CARD_WIDTH - FRAME, 0)
        .into_iter()
        .map(|spans| spans.iter().map(|span| span.content.as_ref()).collect())
        .collect();
    if rows.len() > CARD_LINE_ROWS {
        rows.truncate(CARD_LINE_ROWS);
        let last = rows.pop().unwrap_or_default();
        rows.push(wrap::clip(&format!("{last}\u{2026}"), CARD_WIDTH - FRAME));
    }
    rows
}

/// Rows of a card's box: two edges, the title and the rows of its line.
fn card_height(card: &FlowCard) -> usize {
    3 + card_line_rows(card).len()
}

fn card_box(card: &FlowCard, height: usize) -> Vec<Line<'static>> {
    let border = fg(HAIRLINE);
    let title = Span::styled(printable(card.title.as_str()), bold(TEXT_SOFT));
    let mut lines = vec![
        boxes::top_plain(CARD_WIDTH, border),
        boxes::row(vec![title], CARD_WIDTH, border),
    ];
    for row in card_line_rows(card) {
        lines.push(boxes::row(
            vec![Span::styled(row, dim())],
            CARD_WIDTH,
            border,
        ));
    }
    while lines.len() + 1 < height {
        lines.push(boxes::row(Vec::new(), CARD_WIDTH, border));
    }
    lines.push(boxes::bottom(CARD_WIDTH, border));
    lines
}

/// One row a check: its state as a glyph, its name, and its counts at the right; the
/// detail under it. The head carries the counts of states.
fn draw_checks(checks: &Checks, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(2).max(12);
    let gutter = |spans: Vec<Span<'static>>| {
        let lead = Span::styled(format!("{MARGIN}{GUTTER} "), dim());
        Line::from([vec![lead], spans].concat())
    };
    let mut lines = head("checks", width);
    let summary = wrap::clip(&printable(&checks.summary()), room);
    lines.push(gutter(vec![Span::styled(summary, dim())]));
    for check in checks.items() {
        let (glyph, style) = state_glyph(check.state);
        let counts = check.counts.map_or_else(String::new, counts_text);
        let counts = wrap::clip(&counts, room / 2);
        let side = if counts.is_empty() {
            0
        } else {
            counts.width() + 2
        };
        let name = wrap::clip(
            &printable(check.name.as_str()),
            room.saturating_sub(2 + side),
        );
        let pad = room.saturating_sub(2 + name.width() + counts.width());
        lines.push(gutter(vec![
            Span::styled(format!("{glyph} "), style),
            Span::styled(name, fg(TEXT)),
            Span::raw(" ".repeat(pad)),
            Span::styled(counts, dim()),
        ]));
        if let Some(detail) = &check.detail {
            let detail = wrap::clip(&printable(detail.as_str()), room.saturating_sub(2));
            lines.push(gutter(vec![Span::raw("  "), Span::styled(detail, dim())]));
        }
    }
    lines
}

fn state_glyph(state: CheckState) -> (&'static str, Style) {
    match state {
        CheckState::Queued => (OFF, dim()),
        CheckState::Running => (RUNNING, fg(ACCENT)),
        CheckState::Passed => (DONE, fg(POSITIVE)),
        CheckState::Failed => (FAILED, fg(NEGATIVE)),
        CheckState::Skipped => (SKIPPED, dim()),
    }
}

/// `214/214`, and `3/9 · 2 failed` when some failed.
fn counts_text(counts: Counts) -> String {
    let mut text = format!("{}/{}", counts.passed(), counts.total());
    if counts.failed() > 0 {
        text.push_str(&format!(" {DOT} {} failed", counts.failed()));
    }
    text
}
