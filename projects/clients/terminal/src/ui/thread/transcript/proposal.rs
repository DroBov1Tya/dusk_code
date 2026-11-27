//! A session the agent proposed, as a short card: its title, where it would run, the
//! brief's first lines, and where it stands. The reply row under the box starts or
//! dismisses the newest one waiting. Every text is the agent's, so it is drawn with
//! controls removed.

use dusk_protocol::thread::{Proposal, ProposalState};
use ratatui::text::{Line, Span};

use crate::ui::text::format::one_line;
use crate::ui::thread::{
    theme::{DOT, GUTTER, TEXT_SOFT, TOOL, bold, dim, fg},
    wrap,
};

/// Lines of the brief the card shows; the copy button copies all of it.
const BRIEF_LINES: usize = 3;

/// The card as lines `width` wide, past the entries' left margin.
pub(super) fn render(
    proposal: &Proposal,
    state: &ProposalState,
    width: usize,
) -> Vec<Line<'static>> {
    let room = width.saturating_sub(5).max(1);
    let title = format!("Proposed session {DOT} {}", one_line(&proposal.title));
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("   {TOOL} "), dim()),
        Span::styled(wrap::clip(&title, room), bold(TEXT_SOFT)),
    ])];
    let row = |text: String, style| {
        Line::from(vec![
            Span::styled(format!("   {GUTTER} "), dim()),
            Span::styled(wrap::clip(&text, room), style),
        ])
    };
    let mut about = Vec::new();
    about.extend(
        proposal
            .mode
            .as_deref()
            .map(|mode| format!("mode {}", one_line(mode))),
    );
    about.extend(
        proposal
            .branch
            .as_deref()
            .map(|b| format!("branch {}", one_line(b))),
    );
    about.extend(
        proposal
            .project
            .as_deref()
            .map(|p| format!("in {}", one_line(p))),
    );
    if !about.is_empty() {
        lines.push(row(about.join(&format!(" {DOT} ")), dim()));
    }
    let brief: Vec<&str> = proposal
        .brief
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    for line in brief.iter().take(BRIEF_LINES) {
        lines.push(row(one_line(line), fg(TEXT_SOFT)));
    }
    if brief.len() > BRIEF_LINES {
        let more = format!(
            "{} more lines; the copy button copies the brief",
            brief.len() - BRIEF_LINES
        );
        lines.push(row(more, dim()));
    }
    let said = match state {
        ProposalState::Pending => {
            "Waiting: Alt+1 starts it as a new chat, Alt+2 dismisses it".to_owned()
        }
        ProposalState::Started { thread_id } => format!("Started as chat {thread_id}"),
        ProposalState::Dismissed => "Dismissed".to_owned(),
    };
    lines.push(row(said, dim()));
    lines
}
