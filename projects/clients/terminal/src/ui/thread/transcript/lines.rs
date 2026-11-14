//! A thread's entries laid out as lines.

use super::blocks::{fold_mark, folded, from_agent, indent, plain, tokens, tool, user};
use super::spinner;
use crate::app::{App, Entry, ImageSpot, Tone, Transcript};
use crate::ui::thread::{
    markdown,
    theme::{
        ACCENT, DONE, DOT, FAILED, FOLDED, GUTTER, NEGATIVE, POSITIVE, RAISED, TEXT_MUTED,
        TEXT_SOFT, USER_BAR, bold, dim, fg, thought,
    },
    wrap,
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

/// Image cards sit this far in, under their tool call.
pub(super) const CARD_INDENT: &str = "   ";

/// Lines of a tool result shown when it is open.
pub(super) const MAX_RESULT_LINES: usize = 40;

/// Every entry of a thread as lines, and the line each entry starts on. With `live`,
/// the last entry is still being written; `browsed` is highlighted.
pub fn thread_lines(
    transcript: &Transcript,
    width: usize,
    live: Option<&App>,
    browsed: Option<usize>,
) -> (Vec<Line<'static>>, Vec<usize>) {
    // Agent windows keep the half blocks; their pictures are not placed.
    let mut placed = Placed::default();
    for index in 0..transcript.entries.len() {
        let writing = live.filter(|_| index + 1 == transcript.entries.len());
        append_entry(transcript, index, width, writing, browsed, &mut placed);
    }
    placed.lines.push(Line::default());
    (placed.lines, placed.starts)
}

/// Entries laid out: their lines, the line each starts on, and their image previews.
#[derive(Default)]
pub(super) struct Placed {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) starts: Vec<usize>,
    pub(super) images: Vec<ImageSpot>,
}

/// Adds the entry at `index`, after a blank line unless it continues a folded run.
pub(super) fn append_entry(
    transcript: &Transcript,
    index: usize,
    width: usize,
    live: Option<&App>,
    browsed: Option<usize>,
    placed: &mut Placed,
) {
    let text_width = width.saturating_sub(4).max(10);
    let entries = &transcript.entries;
    let entry = &entries[index];
    let folded_run = index > 0 && is_meta(&entries[index - 1]) && is_meta(entry);
    if index > 0 && !folded_run {
        placed.lines.push(Line::default());
    }
    placed.starts.push(placed.lines.len());
    let mut previews = Vec::new();
    let folded_away = transcript.is_folded_away(index);
    let mut drawn = if folded_away {
        Vec::new()
    } else if matches!(entry, Entry::Activity { .. }) {
        activity(transcript, index)
    } else if transcript.is_hidden(index) {
        let text = transcript.text(index).unwrap_or_default();
        vec![hidden_line(entry, &text)]
    } else {
        entry_lines(entry, text_width, live, ends_turn(entries, index))
    };
    // A picture stays in the thread whether or not its step is folded.
    if transcript.images.enabled()
        && let Entry::Tool {
            call,
            result: Some(result),
            ..
        } = entry
        && call.name == "display_image"
        && !result.is_error
    {
        for artifact in &result.artifacts {
            let (card, preview) =
                crate::ui::text::image_card::render(transcript, artifact, text_width);
            if let Some(preview) = preview {
                previews.push((drawn.len() + preview.row, preview, artifact.clone()));
            }
            drawn.extend(indent(card, CARD_INDENT));
        }
    }
    if browsed == Some(index)
        && let Some(header) = drawn.first_mut()
    {
        *header = std::mem::take(header).style(Style::new().bg(RAISED));
    }
    let first = placed.lines.len();
    placed.images.extend(
        previews
            .into_iter()
            .map(|(row, preview, artifact)| ImageSpot {
                line: first + row,
                column: CARD_INDENT.len() as u16,
                width: preview.width,
                height: preview.height,
                artifact,
            }),
    );
    placed.lines.extend(drawn);
}

/// A run in one line: what goes on while it is the newest thing in the thread, then how
/// many steps it took.
pub(super) fn activity(transcript: &Transcript, index: usize) -> Vec<Line<'static>> {
    let open = matches!(transcript.entries[index], Entry::Activity { open: true });
    let mark = Span::styled(format!("   {} ", fold_mark(open)), fg(TEXT_MUTED));
    if let Some(step) = transcript.current_step(index) {
        return vec![Line::from(vec![mark, Span::styled(step, bold(ACCENT))])];
    }
    let steps = transcript.run(index).count();
    let failed = transcript
        .run(index)
        .filter(
            |entry| matches!(entry, Entry::Tool { result: Some(result), .. } if result.is_error),
        )
        .count();
    let said = match steps {
        1 => "1 step".to_owned(),
        steps => format!("{steps} steps"),
    };
    let mut spans = vec![mark, Span::styled(said, thought())];
    if failed > 0 {
        spans.push(Span::styled(
            format!("  {FAILED} {failed} failed"),
            fg(NEGATIVE),
        ));
    }
    vec![Line::from(spans)]
}

/// What a hidden entry leaves: what it was and how long, to show it again.
pub(super) fn hidden_line(entry: &Entry, text: &str) -> Line<'static> {
    let what = match entry {
        Entry::User { .. } => "your message",
        Entry::Answer(_) => "answer",
        Entry::Activity { .. } => "activity",
        Entry::Reasoning { .. } => "thinking",
        Entry::Tool { .. } => "tool call",
        Entry::Summary { .. } => "summary",
        Entry::FromAgent { .. } => "agent message",
        Entry::Notice { .. } => "notice",
    };
    let lines = text.lines().count().max(1);
    Line::from(vec![
        Span::styled(format!("   {FOLDED} "), dim()),
        Span::styled(format!("{what} hidden {DOT} {lines} lines"), dim()),
    ])
}

/// Whether the answer at `index` is the last word of its turn: no tool call follows it.
pub(super) fn ends_turn(entries: &[Entry], index: usize) -> bool {
    let next = entries[index + 1..].iter().find(|entry| {
        !matches!(
            entry,
            Entry::Activity { .. } | Entry::Reasoning { .. } | Entry::Notice { .. }
        )
    });
    matches!(entries[index], Entry::Answer(_)) && !matches!(next, Some(Entry::Tool { .. }))
}

/// `live` is set while the entry is still being written.
pub(super) fn entry_lines(
    entry: &Entry,
    width: usize,
    live: Option<&App>,
    ends_turn: bool,
) -> Vec<Line<'static>> {
    match entry {
        Entry::User { text, open } => user(text, *open, width),
        Entry::Answer(text) if ends_turn => markdown::render(text.trim_end(), width)
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::styled(format!(" {USER_BAR} "), fg(ACCENT))];
                spans.extend(line.spans);
                Line::from(spans).style(line.style)
            })
            .collect(),
        Entry::Answer(text) => indent(markdown::render(text.trim_end(), width), "   "),
        Entry::Reasoning { text, open } => {
            let marker = live.map_or(fold_mark(*open), |app| spinner(app));
            let header = Line::from(vec![
                Span::styled(format!("   {marker} "), thought()),
                Span::styled("thinking", thought()),
            ]);
            folded(header, *open, || {
                let body = plain(text.trim(), width.saturating_sub(2), thought());
                indent(body, &format!("   {GUTTER} "))
            })
        }
        Entry::Tool { call, result, open } => tool(call, result.as_ref(), *open, width, live),
        // Drawn from the whole run in `append_entry`.
        Entry::Activity { .. } => Vec::new(),
        Entry::Summary { text, open } => {
            let header = Line::from(vec![
                Span::styled(format!("   {} ", fold_mark(*open)), fg(TEXT_SOFT)),
                Span::styled("summary of the earlier conversation", fg(TEXT_SOFT)),
                Span::styled(format!(" {DOT} ~{} tokens", tokens(text)), dim()),
            ]);
            folded(header, *open, || {
                let body = markdown::render(text, width.saturating_sub(2));
                indent(body, &format!("   {GUTTER} "))
            })
        }
        Entry::FromAgent {
            thread_id,
            text,
            open,
        } => from_agent(thread_id, text, *open, width),
        Entry::Notice { text, tone, .. } => {
            let (mark, style) = match tone {
                Tone::Info => (DOT, dim()),
                Tone::Error => (FAILED, fg(NEGATIVE)),
                Tone::Success => (DONE, bold(POSITIVE)),
            };
            // Each line of a notice wraps on its own, so a line break stays a line break.
            text.lines()
                .flat_map(|line| {
                    let spans = vec![Span::styled(line.to_owned(), style)];
                    wrap::words(spans, width.saturating_sub(2), 0)
                })
                .enumerate()
                .map(|(row, spans)| {
                    let lead = if row == 0 {
                        format!("   {mark} ")
                    } else {
                        "     ".into()
                    };
                    Line::from([vec![Span::styled(lead, style)], spans].concat())
                })
                .collect()
        }
    }
}

pub(super) fn is_meta(entry: &Entry) -> bool {
    matches!(
        entry,
        Entry::Activity { .. } | Entry::Reasoning { .. } | Entry::Tool { .. }
    )
}
