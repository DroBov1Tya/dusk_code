//! A diagram as plain text: what the model reads back from `draw_diagram`, and what a client
//! without a drawing for it shows. It reads in the order the picture does.

use super::layout::reading_order;
use super::spec::{Diagram, DiagramKind, EdgeStyle};

pub(super) fn write(diagram: &Diagram, text: &mut String) {
    text.push_str(diagram.kind().name());
    if let Some(title) = diagram.title() {
        text.push_str(&format!(": {title}"));
    }
    text.push('\n');
    let nodes = diagram.nodes();
    let ends = diagram.ends();
    let arrow = |style: EdgeStyle| match style {
        EdgeStyle::Dashed => "-->",
        EdgeStyle::Solid | EdgeStyle::Strong => "->",
    };
    let words = |edge: usize| {
        diagram.edges()[edge]
            .label
            .as_ref()
            .map_or(String::new(), |label| format!(": {label}"))
    };
    match diagram.kind() {
        DiagramKind::Sequence => {
            let names: Vec<&str> = nodes.iter().map(|node| node.label()).collect();
            text.push_str(&format!("participants: {}\n", names.join(", ")));
            for (edge, &(from, to)) in ends.iter().enumerate() {
                let style = diagram.edges()[edge].style();
                text.push_str(&format!(
                    "{}. {} {} {}{}\n",
                    edge + 1,
                    nodes[from].label(),
                    arrow(style),
                    nodes[to].label(),
                    words(edge)
                ));
            }
        }
        DiagramKind::Timeline => {
            for node in nodes {
                if let Some(when) = &node.when {
                    text.push_str(&format!("{when}: "));
                }
                text.push_str(node.label());
                if let Some(note) = &node.note {
                    text.push_str(&format!(" - {note}"));
                }
                text.push('\n');
            }
        }
        DiagramKind::Tree => {
            let mut has_parent = vec![false; nodes.len()];
            for &(_, to) in &ends {
                has_parent[to] = true;
            }
            let mut walk: Vec<(usize, usize, Option<usize>)> = (0..nodes.len())
                .rev()
                .filter(|&node| !has_parent[node])
                .map(|node| (node, 0, None))
                .collect();
            while let Some((node, depth, edge)) = walk.pop() {
                text.push_str(&"  ".repeat(depth));
                text.push_str("- ");
                text.push_str(nodes[node].label());
                if let Some(note) = &nodes[node].note {
                    text.push_str(&format!(" ({note})"));
                }
                if let Some(edge) = edge {
                    text.push_str(&words(edge).replacen(':', " -", 1));
                }
                text.push('\n');
                for (child_edge, &(from, to)) in ends.iter().enumerate().rev() {
                    if from == node {
                        walk.push((to, depth + 1, Some(child_edge)));
                    }
                }
            }
        }
        DiagramKind::Flow | DiagramKind::Architecture | DiagramKind::State => {
            for node in reading_order(diagram) {
                let own = &nodes[node];
                text.push_str(own.label());
                if let Some(note) = &own.note {
                    text.push_str(&format!(" ({note})"));
                }
                if let Some(group) = diagram.group_of(node) {
                    text.push_str(&format!(" [{}]", diagram.groups()[group].label));
                }
                text.push('\n');
                for (edge, &(from, to)) in ends.iter().enumerate() {
                    if from == node {
                        let style = diagram.edges()[edge].style();
                        text.push_str(&format!(
                            "  {} {}{}\n",
                            arrow(style),
                            nodes[to].label(),
                            words(edge)
                        ));
                    }
                }
            }
        }
    }
    let legend: Vec<String> = diagram
        .legend()
        .iter()
        .map(|entry| {
            let key = entry
                .tone
                .map(|tone| tone.name())
                .or(entry.style.map(|style| style.name()))
                .unwrap_or_default();
            format!("{key} = {}", entry.label)
        })
        .collect();
    if !legend.is_empty() {
        text.push_str(&format!("legend: {}\n", legend.join(", ")));
    }
}
