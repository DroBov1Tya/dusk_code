//! Blocks the agent showed with the `show` tool. Every text in one is a protocol `Line`:
//! drawn as it is, never as markdown or a link. Each block says it is the agent's, so it
//! cannot pass for a prompt of the app.

use dusk_protocol::thread::Block;
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::blocks::{indent, verbatim};
use crate::ui::text::format::printable;
use crate::ui::thread::{
    theme::{DOT, GUTTER, TEXT_SOFT, TOOL, dim, fg},
    wrap,
};

/// A block as lines `width` wide, past the entries' left margin.
pub(super) fn render(block: &Block, width: usize) -> Vec<Line<'static>> {
    match block {
        Block::Flow(_) => as_text(block, "flow", width),
        Block::Checks(_) => as_text(block, "checks", width),
        Block::Choices(_) => as_text(block, "choices", width),
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
fn head(kind: &str, width: usize) -> Vec<Line<'static>> {
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
