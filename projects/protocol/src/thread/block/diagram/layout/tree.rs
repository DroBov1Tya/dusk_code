//! A tree as an outline, for a column too narrow for its layers: each node on a row of its
//! own, indented under its parent, joined to it by an elbow from the parent's left side.

use super::geometry::{Point, Rect, rounded};
use super::{
    Anchor, EdgePath, Layout, Measure, NodeBox, Owner, Sized, Text, TextStyle, edge_label,
    node_texts, size,
};
use crate::thread::block::diagram::spec::Diagram;

/// How far a child stands in from its parent, where the elbow leaves the parent, and the
/// space between two rows.
const INDENT: f32 = 28.;
const ELBOW: f32 = 14.;
const ROW_GAP: f32 = 8.;

pub(super) fn outline(diagram: &Diagram, measure: &dyn Measure, wrap: f32) -> Layout {
    let count = diagram.nodes().len();
    let ends = diagram.ends();
    let mut children: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    let mut has_parent = vec![false; count];
    for (edge, &(from, to)) in ends.iter().enumerate() {
        children[from].push((edge, to));
        has_parent[to] = true;
    }
    // Rows in the order a reader goes through the tree: a node, then its children.
    let mut rows: Vec<(usize, usize, Option<usize>)> = Vec::new();
    let mut walk: Vec<(usize, usize, Option<usize>)> = (0..count)
        .filter(|&node| !has_parent[node])
        .rev()
        .map(|node| (node, 0, None))
        .collect();
    while let Some((node, depth, edge)) = walk.pop() {
        rows.push((node, depth, edge));
        for &(edge, child) in children[node].iter().rev() {
            walk.push((child, depth + 1, Some(edge)));
        }
    }
    let sized: Vec<Sized> = diagram
        .nodes()
        .iter()
        .map(|node| size(measure, node, wrap))
        .collect();
    let mut laid = Layout::empty();
    let mut rects = vec![Rect::default(); count];
    let mut top = 0.;
    for &(node, depth, _) in &rows {
        let rect = Rect::new(depth as f32 * INDENT, top, sized[node].w, sized[node].h);
        rects[node] = rect;
        top += rect.h + ROW_GAP;
        laid.nodes.push(NodeBox {
            node,
            rect,
            figure: sized[node].figure,
            tone: diagram.nodes()[node].tone,
        });
        node_texts(measure, node, &sized[node], rect, &mut laid.texts);
    }
    laid.nodes.sort_by_key(|node| node.node);
    for &(node, _, edge) in &rows {
        let Some(edge) = edge else { continue };
        let parent = ends[edge].0;
        let (from, to) = (rects[parent], rects[node]);
        let x = from.x + ELBOW;
        let points = [
            Point::new(x, from.bottom()),
            Point::new(x, to.center().y),
            Point::new(to.x, to.center().y),
        ];
        let label = diagram.edges()[edge].label.as_ref().map(|label| {
            let (lines, w, h) = edge_label(measure, label.as_str(), wrap);
            let rect = Rect::new(to.right() + 6., to.center().y - h / 2., w, h);
            let line = TextStyle::Edge.line_height();
            for (at, text) in lines.iter().enumerate() {
                laid.texts.push(Text {
                    text: text.clone(),
                    x: rect.x + 4.,
                    y: rect.center().y - lines.len() as f32 * line / 2. + at as f32 * line,
                    width: measure.width(text, TextStyle::Edge),
                    anchor: Anchor::Start,
                    style: TextStyle::Edge,
                    owner: Owner::Edge(edge),
                });
            }
            rect
        });
        laid.edges.push(EdgePath {
            edge,
            from: parent,
            to: node,
            segs: rounded(&points, 6.),
            head: None,
            style: diagram.edges()[edge].style(),
            label,
        });
    }
    laid.edges.sort_by_key(|edge| edge.edge);
    laid.fit()
}
