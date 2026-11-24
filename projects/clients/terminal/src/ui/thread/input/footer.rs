//! The line under the input: mode, git and status.

use super::InputView;
use crate::app::{Activity, App, Focus};
use crate::ui::thread::{
    theme::{ACCENT, DEEP, DUSK, NEGATIVE, TEXT_MUTED, TEXT_SOFT, WARNING, bold, dim, fg},
    transcript::spinner,
};
use dusk_protocol::config::ApprovalPolicy;
use dusk_protocol::project::GitSummary;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// The chat's mode as a cell of its own colour, amber for MAIN and violet for the
/// others; Tab switches it.
pub(super) fn mode_chip(mode: &str) -> Span<'static> {
    let color = if mode == "MAIN" { ACCENT } else { DUSK };
    Span::styled(format!(" {} ", mode.to_lowercase()), bold(DEEP).bg(color))
}

/// The repository, branch, commits, changed files and the distance to the upstream;
/// `worktree` when it is the worktree of the thread's own branch.
pub(super) fn git_line(summary: &GitSummary, worktree: bool) -> Line<'static> {
    let dot = || Span::styled(" \u{00B7} ", dim());
    let mut spans = vec![
        Span::styled(format!("  {}", summary.repository), dim()),
        dot(),
        Span::styled(
            summary.branch.clone().unwrap_or_else(|| "detached".into()),
            fg(TEXT_SOFT),
        ),
        dot(),
    ];
    if worktree {
        spans.extend([Span::styled("worktree", fg(ACCENT)), dot()]);
    }
    spans.extend([
        Span::styled(format!("{} commits", summary.commits), dim()),
        dot(),
    ]);
    spans.push(match summary.changed {
        0 => Span::styled("clean", dim()),
        changed => Span::styled(format!("{changed} changed"), fg(TEXT_SOFT)),
    });
    if let Some((ahead, behind)) = summary.ahead_behind {
        spans.push(dot());
        spans.push(Span::styled(
            format!("\u{2191}{ahead} \u{2193}{behind}"),
            if ahead + behind == 0 {
                dim()
            } else {
                fg(TEXT_SOFT)
            },
        ));
    }
    Line::from(spans)
}

/// The star of a thinking turn, shape after shape and back.
const STAR: [&str; 10] = [
    "\u{00B7}", "\u{2722}", "\u{2733}", "\u{2736}", "\u{273B}", "\u{273D}", "\u{273B}", "\u{2736}",
    "\u{2733}", "\u{2722}",
];

/// The mark, colour and words of what the agent does now. The protocol chooses the kind
/// of work, as for every client; here each kind has its mark: a star that changes shape
/// while the turn thinks, the spinner for work that runs, and a mark that stands still
/// while the text itself arrives, the turn waits for the user or the provider holds it.
fn doing(app: &App, activity: &Activity) -> (&'static str, ratatui::style::Color, String) {
    use dusk_protocol::thread::WorkKind;
    // What the thread itself never says: a fork being described.
    if *activity == Activity::Forking {
        let what = "describing the thread for a fork".to_owned();
        return (spinner(app), DUSK, what);
    }
    let now = dusk_protocol::system::now_ms();
    let Some(work) = app.transcript.work(now) else {
        // The message is sent and the turn's first event is on its way.
        return (
            STAR[(app.frame / 3) % STAR.len()],
            ACCENT,
            app.thinking_label().to_owned(),
        );
    };
    let words = work.label.to_lowercase();
    match work.kind {
        WorkKind::Thinking => (
            STAR[(app.frame / 3) % STAR.len()],
            if work.stalled { WARNING } else { ACCENT },
            format!("{}{}", app.thinking_label(), app.rate_note()),
        ),
        WorkKind::Writing => ("\u{00B7}", ACCENT, format!("{words}{}", app.rate_note())),
        WorkKind::Planning | WorkKind::Steps | WorkKind::Agents { .. } => {
            (spinner(app), TEXT_SOFT, words)
        }
        WorkKind::WaitingForUser => ("\u{25CB}", WARNING, words),
        WorkKind::Compacting => (spinner(app), DUSK, words),
        // A wait's and a retry's words are the protocol's, as written.
        WorkKind::Held => ("\u{25CC}", NEGATIVE, work.label),
    }
}

/// The rule: what the agent is doing and how far the transcript is scrolled.
/// What goes on the card's top edge, and the note for its right end.
pub(super) fn status(app: &App) -> (Vec<Span<'static>>, String) {
    let doing = match (&app.activity, app.focus) {
        (Activity::Idle, Focus::Browse(_)) => {
            vec![Span::styled("up/down move  enter fold  esc back", dim())]
        }
        (Activity::Idle, Focus::Input) => {
            let state = if app.provider_available {
                "ready"
            } else {
                "provider unavailable"
            };
            let saved = if app.local_unsent.is_empty() {
                String::new()
            } else {
                format!("  {} unsent \u{00B7} /resend", app.local_unsent.len())
            };
            vec![Span::styled(
                format!("{state}{saved}  /  F2 sidebar"),
                dim(),
            )]
        }
        (activity, _) => {
            let (mark, color, what) = doing(app, activity);
            // How long the turn has run and what it used, beside what it does now.
            let mut took = app
                .turn_started_at
                .map(|started| {
                    let ms = dusk_protocol::system::now_ms() - started;
                    format!(" \u{00B7} {}", dusk_protocol::thread::duration_words(ms))
                })
                .unwrap_or_default();
            if let Some(spend) = app.turn_spend.filter(|_| app.turn_started_at.is_some()) {
                if let Some(tokens) = spend.tokens() {
                    took.push_str(&format!(" \u{00B7} {tokens} tok"));
                }
                took.push_str(&format!(" \u{00B7} {}", spend.step()));
            }
            vec![
                Span::styled(format!("{mark} "), fg(color)),
                Span::styled(what, fg(color)),
                Span::styled(took, fg(TEXT_MUTED)),
            ]
        }
    };
    let below = match (&app.flash, app.scroll) {
        (Some(flash), _) => flash.clone(),
        (None, 0) => String::new(),
        (None, scroll) => format!("{scroll} lines below"),
    };
    let mut spans = vec![Span::raw(" ")];
    if !app.mode.is_empty() {
        spans.push(mode_chip(&app.mode));
        spans.push(Span::raw("  "));
    }
    spans.extend(doing);
    spans.push(Span::raw(" "));
    (spans, below)
}

impl InputView<'_> {
    /// The repository summary and package version share a row below the message card.
    pub(super) fn draw_footer(
        &self,
        frame: &mut Frame,
        area: Rect,
        git: Option<&GitSummary>,
        worktree: bool,
        folder: &str,
        approvals: ApprovalPolicy,
    ) {
        if area.is_empty() {
            return;
        }
        let bottom = Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        };
        // A status bar of its own colour, so the row reads as one even with little on it.
        frame.render_widget(Block::new().style(Style::new().bg(DEEP)), bottom);
        const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
        // Keep the version whole; an extremely narrow terminal has no room for it.
        let version_width = VERSION.width() as u16;
        let reserved = if bottom.width >= version_width {
            frame.render_widget(
                Paragraph::new(Span::styled(VERSION, dim())).right_aligned(),
                bottom,
            );
            version_width.saturating_add(1)
        } else {
            0
        };
        let mut line = match git {
            Some(summary) => git_line(summary, worktree),
            None => Line::from(vec![
                Span::styled(format!("  {folder}"), dim()),
                Span::styled(" \u{00B7} not a git repository", dim()),
            ]),
        };
        // First, so a narrow terminal cuts the folder rather than the warning.
        let color = match approvals {
            ApprovalPolicy::Ask => None,
            ApprovalPolicy::FullAccess => Some(NEGATIVE),
            _ => Some(ACCENT),
        };
        if let Some(color) = color {
            let label = format!(" {} ", approvals.name());
            line.spans.insert(0, Span::styled(label, bold(color)));
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                width: bottom.width.saturating_sub(reserved),
                ..bottom
            },
        );
    }
}
