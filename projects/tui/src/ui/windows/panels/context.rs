//! /context: what the model reads, section by section.

use ratatui::text::{Line, Span};

use dusk_core::message::commands::ContextReport;

use crate::app::compact_number;
use crate::ui::windows::theme::{TEXT, bold, dim};

/// The context by part, each with its share of the limit, then what it is made of.
pub fn context(report: &ContextReport) -> Vec<Line<'static>> {
    let share = |tokens: u64| {
        let percent = tokens.saturating_mul(100) / report.limit.max(1);
        format!("{:>6}  {percent:>3}%", compact_number(tokens))
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(" in context", bold(TEXT)),
            Span::raw("\t"),
            Span::styled(
                format!(
                    "{} of {}  ",
                    compact_number(report.total),
                    compact_number(report.limit)
                ),
                bold(TEXT),
            ),
        ]),
        Line::from(Span::styled(
            " parts are estimates; the total is the provider's count when known",
            dim(),
        )),
    ];
    for section in &report.sections {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled(format!(" {}", section.name), bold(TEXT)),
            Span::raw("\t"),
            Span::styled(format!("{}  ", share(section.tokens)), bold(TEXT)),
        ]));
        if section.items.is_empty() {
            lines.push(Line::from(Span::styled("   none", dim())));
        }
        for (name, tokens) in &section.items {
            lines.push(Line::from(vec![
                Span::raw(format!("   {name}")),
                Span::raw("\t"),
                Span::styled(format!("{}  ", share(*tokens)), dim()),
            ]));
        }
    }
    lines
}
