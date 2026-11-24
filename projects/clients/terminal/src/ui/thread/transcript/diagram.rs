//! A diagram in the terminal, which has no pixels for the protocol's layout: drawn as text
//! in the order the picture reads. A layered diagram lists its nodes layer by layer, each
//! with the edges it sends; a tree branches with box lines; a sequence numbers its messages;
//! a timeline runs down a rail. Every word is a protocol `Line`, drawn as it is.

use dusk_protocol::thread::diagram::{
    Diagram, DiagramKind, DiagramTone, EdgeStyle, layout::reading_order,
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::shown::head;
use crate::ui::text::{format::printable, wrap};
use crate::ui::thread::theme::{
    ACCENT, DOT, GUTTER, INFO, NEGATIVE, OFF, ON, POSITIVE, TEXT, TEXT_MUTED, WARNING, bold, dim,
    fg,
};

/// What the thread keeps free on the left of a block, and the gutter after it.
const LEAD: &str = "   ";

/// One row of the drawing: what stands before its words on its first line and on the
/// lines it wraps to (spaces, or a tree's box lines), then the words.
struct Row {
    first: String,
    rest: String,
    spans: Vec<Span<'static>>,
}

/// A row indented by `indent` spaces.
fn row(indent: usize, spans: Vec<Span<'static>>) -> Row {
    let pad = " ".repeat(indent);
    Row {
        first: pad.clone(),
        rest: format!("{pad}  "),
        spans,
    }
}

pub(super) fn render(diagram: &Diagram, width: usize) -> Vec<Line<'static>> {
    let mut lines = head(&format!("diagram {DOT} {}", diagram.kind().name()), width);
    let room = width.saturating_sub(LEAD.len() + 2).max(12);
    let mut rows: Vec<Row> = Vec::new();
    if let Some(title) = diagram.title() {
        rows.push(row(0, vec![Span::styled(printable(title), bold(TEXT))]));
    }
    match diagram.kind() {
        DiagramKind::Sequence => sequence(diagram, &mut rows),
        DiagramKind::Timeline => timeline(diagram, &mut rows),
        DiagramKind::Tree => tree(diagram, &mut rows),
        _ => layered(diagram, &mut rows),
    }
    let legend: Vec<String> = diagram
        .legend()
        .iter()
        .map(|entry| {
            let key = match (entry.tone, entry.style) {
                (Some(tone), _) => tone.name().to_owned(),
                (None, style) => arrow(style.unwrap_or_default()).to_owned(),
            };
            format!("{key} {}", printable(entry.label.as_str()))
        })
        .collect();
    if !legend.is_empty() {
        rows.push(row(
            0,
            vec![Span::styled(legend.join(&format!("  {DOT} ")), dim())],
        ));
    }
    for row in rows {
        let before = row.first.chars().count().max(row.rest.chars().count());
        for (at, spans) in wrap::words(row.spans, room.saturating_sub(before), 0)
            .into_iter()
            .enumerate()
        {
            let prefix = if at == 0 { &row.first } else { &row.rest };
            let lead = format!("{LEAD}{GUTTER} {prefix}");
            lines.push(Line::from(
                [vec![Span::styled(lead, dim())], spans].concat(),
            ));
        }
    }
    lines
}

fn tone_style(tone: Option<DiagramTone>) -> Style {
    match tone {
        None => fg(TEXT_MUTED),
        Some(DiagramTone::Accent) => fg(ACCENT),
        Some(DiagramTone::Info) => fg(INFO),
        Some(DiagramTone::Success) => fg(POSITIVE),
        Some(DiagramTone::Warning) => fg(WARNING),
        Some(DiagramTone::Danger) => fg(NEGATIVE),
        Some(DiagramTone::Muted) => dim(),
    }
}

fn arrow(style: EdgeStyle) -> &'static str {
    match style {
        EdgeStyle::Dashed => "\u{21E2}",
        EdgeStyle::Solid | EdgeStyle::Strong => "\u{2192}",
    }
}

/// A node's name and its note after it, quieter.
fn named(diagram: &Diagram, node: usize) -> Vec<Span<'static>> {
    let own = &diagram.nodes()[node];
    let mut spans = vec![Span::styled(printable(own.label()), fg(TEXT))];
    if let Some(note) = &own.note {
        spans.push(Span::styled(
            format!("  {}", printable(note.as_str())),
            dim(),
        ));
    }
    spans
}

fn edge_words(diagram: &Diagram, edge: usize) -> Option<Span<'static>> {
    let label = diagram.edges()[edge].label.as_ref()?;
    Some(Span::styled(
        format!("  {}", printable(label.as_str())),
        dim(),
    ))
}

/// Each node in reading order, marked in its tone, with the edges it sends under it.
fn layered(diagram: &Diagram, rows: &mut Vec<Row>) {
    let ends = diagram.ends();
    let mut group = None;
    for node in reading_order(diagram) {
        let own = &diagram.nodes()[node];
        let in_group = diagram.group_of(node);
        if in_group != group {
            if let Some(at) = in_group {
                let name = diagram.groups()[at].label.as_str().to_uppercase();
                rows.push(row(0, vec![Span::styled(printable(&name), dim())]));
            }
            group = in_group;
        }
        let indent = usize::from(in_group.is_some()) * 2;
        let mark = if own.shape().is_marker() { OFF } else { ON };
        let mut spans = vec![Span::styled(format!("{mark} "), tone_style(own.tone))];
        spans.extend(named(diagram, node));
        rows.push(row(indent, spans));
        for (edge, &(from, to)) in ends.iter().enumerate() {
            if from != node {
                continue;
            }
            let style = diagram.edges()[edge].style();
            let color = if style == EdgeStyle::Strong {
                fg(ACCENT)
            } else {
                dim()
            };
            let mut spans = vec![
                Span::styled(format!("{} ", arrow(style)), color),
                Span::styled(printable(diagram.nodes()[to].label()), fg(TEXT)),
            ];
            spans.extend(edge_words(diagram, edge));
            rows.push(row(indent + 2, spans));
        }
    }
}

/// Box lines from each parent to its children.
fn tree(diagram: &Diagram, rows: &mut Vec<Row>) {
    let ends = diagram.ends();
    let count = diagram.nodes().len();
    let mut children: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    let mut has_parent = vec![false; count];
    for (edge, &(from, to)) in ends.iter().enumerate() {
        children[from].push((edge, to));
        has_parent[to] = true;
    }
    // The walk carries each node's prefix of box lines.
    let mut walk: Vec<(usize, String, String, Option<usize>)> = (0..count)
        .rev()
        .filter(|&node| !has_parent[node])
        .map(|node| (node, String::new(), String::new(), None))
        .collect();
    while let Some((node, branch, below, edge)) = walk.pop() {
        let own = &diagram.nodes()[node];
        let mut spans = Vec::new();
        if own.tone.is_some() {
            spans.push(Span::styled(format!("{ON} "), tone_style(own.tone)));
        }
        spans.extend(named(diagram, node));
        if let Some(edge) = edge {
            spans.extend(edge_words(diagram, edge));
        }
        rows.push(Row {
            first: branch,
            rest: below.clone(),
            spans,
        });
        let kids = &children[node];
        for (at, &(edge, child)) in kids.iter().enumerate().rev() {
            let last = at + 1 == kids.len();
            let branch = format!(
                "{below}{}",
                if last {
                    "\u{2514}\u{2500} "
                } else {
                    "\u{251C}\u{2500} "
                }
            );
            let next = format!("{below}{}", if last { "   " } else { "\u{2502}  " });
            walk.push((child, branch, next, Some(edge)));
        }
    }
}

/// The participants, then each message numbered, in order.
fn sequence(diagram: &Diagram, rows: &mut Vec<Row>) {
    let names: Vec<String> = diagram
        .nodes()
        .iter()
        .map(|node| printable(node.label()))
        .collect();
    rows.push(row(
        0,
        vec![Span::styled(names.join(&format!(" {DOT} ")), dim())],
    ));
    for (edge, &(from, to)) in diagram.ends().iter().enumerate() {
        let style = diagram.edges()[edge].style();
        let color = if style == EdgeStyle::Strong {
            fg(ACCENT)
        } else {
            dim()
        };
        let mut spans = vec![
            Span::styled(format!("{:>2}  ", edge + 1), dim()),
            Span::styled(names[from].clone(), fg(TEXT)),
            Span::styled(format!(" {} ", arrow(style)), color),
            Span::styled(names[to].clone(), fg(TEXT)),
        ];
        spans.extend(edge_words(diagram, edge));
        rows.push(row(0, spans));
    }
}

/// Events down a rail: the date, a dot in the event's tone, its words.
fn timeline(diagram: &Diagram, rows: &mut Vec<Row>) {
    let dates: Vec<String> = diagram
        .nodes()
        .iter()
        .map(|node| {
            node.when
                .as_ref()
                .map_or_else(String::new, |w| printable(w.as_str()))
        })
        .collect();
    let room = dates.iter().map(|d| d.chars().count()).max().unwrap_or(0);
    for (node, date) in dates.iter().enumerate() {
        let own = &diagram.nodes()[node];
        let mut spans = vec![
            Span::styled(format!("{date:>room$} "), dim()),
            Span::styled(
                format!("{ON} "),
                tone_style(own.tone.or(Some(DiagramTone::Muted))),
            ),
        ];
        spans.extend(named(diagram, node));
        rows.push(row(0, spans));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(diagram: serde_json::Value, width: usize) -> Vec<String> {
        let diagram = serde_json::from_value(diagram).expect("a diagram within caps");
        render(&diagram, width)
            .into_iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn a_tree_branches_with_box_lines_and_a_sequence_numbers_its_messages() {
        let tree = drawn(
            serde_json::json!({ "kind": "tree", "nodes": [{ "id": "src" }, { "id": "a" }, { "id": "b" }],
              "edges": [{ "from": "src", "to": "a" }, { "from": "src", "to": "b" }] }),
            60,
        );
        assert!(
            tree.iter().any(|l| l.ends_with("\u{251C}\u{2500} a")),
            "{tree:#?}"
        );
        assert!(
            tree.iter().any(|l| l.ends_with("\u{2514}\u{2500} b")),
            "{tree:#?}"
        );
        let deep = drawn(
            serde_json::json!({ "kind": "tree", "nodes": [{ "id": "r" }, { "id": "a" }, { "id": "b" }, { "id": "c" }],
              "edges": [{ "from": "r", "to": "a" }, { "from": "a", "to": "b" }, { "from": "r", "to": "c" }] }),
            60,
        );
        assert!(
            deep.iter()
                .any(|l| l.ends_with("\u{2502}  \u{2514}\u{2500} b")),
            "{deep:#?}"
        );
        let sequence = drawn(
            serde_json::json!({ "kind": "sequence", "nodes": [{ "id": "c" }, { "id": "s" }],
              "edges": [{ "from": "c", "to": "s", "label": "GET /" },
                        { "from": "s", "to": "c", "style": "dashed" }] }),
            60,
        );
        assert!(
            sequence
                .iter()
                .any(|l| l.ends_with("1  c \u{2192} s  GET /")),
            "{sequence:#?}"
        );
        assert!(
            sequence.iter().any(|l| l.ends_with("2  s \u{21E2} c")),
            "{sequence:#?}"
        );
    }

    #[test]
    fn a_narrow_terminal_wraps_every_row_inside_its_width() {
        let flow = drawn(
            serde_json::json!({ "kind": "flow", "title": "A rather long title that has to wrap",
              "nodes": [{ "id": "a", "label": "A step with a long label", "note": "and a note" },
                        { "id": "b" }],
              "edges": [{ "from": "a", "to": "b", "label": "then afterwards" }] }),
            28,
        );
        // The head is the one every block has; the diagram's own rows keep to the width.
        for line in flow.iter().filter(|line| line.contains(GUTTER)) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= 28,
                "{line:?}"
            );
        }
        assert!(flow.iter().any(|l| l.contains("\u{2192} b")), "{flow:#?}");
    }
}
