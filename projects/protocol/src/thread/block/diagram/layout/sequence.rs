//! A sequence: the participants in a row of boxes, a dashed lifeline down from each, and
//! the messages as arrows between lifelines, one row each, top to bottom. The space
//! between two lifelines grows until every message's words fit between the lines they
//! join, so a label does not sit over a lifeline when it can be helped.

use super::geometry::{Point, Rect};
use super::{
    Guide, GuideKind, HEAD_LENGTH, Head, Layout, Measure, NodeBox, Sized, edge_label, edge_texts,
    node_texts, size,
};
use crate::thread::block::diagram::layout::EdgePath;
use crate::thread::block::diagram::spec::Diagram;

/// Least space between two participants' boxes.
const GAP: f32 = 24.;
/// Space between the boxes and the first message, and between two messages.
const FIRST: f32 = 20.;
const ROW: f32 = 18.;
/// How far a message to oneself stands out and how tall it is.
const LOOP_W: f32 = 24.;
const LOOP_H: f32 = 20.;

pub(super) fn layout(diagram: &Diagram, measure: &dyn Measure, wrap: f32) -> Layout {
    let sized: Vec<Sized> = diagram
        .nodes()
        .iter()
        .map(|node| size(measure, node, wrap))
        .collect();
    let count = sized.len();
    let ends = diagram.ends();
    let labels: Vec<Option<(Vec<String>, f32, f32)>> = diagram
        .edges()
        .iter()
        .map(|edge| {
            edge.label
                .as_ref()
                .map(|label| edge_label(measure, label.as_str(), wrap))
        })
        .collect();

    let mut gaps: Vec<f32> = (1..count)
        .map(|at| (sized[at - 1].w + sized[at].w) / 2. + GAP)
        .collect();
    let mut order: Vec<usize> = (0..ends.len()).collect();
    order.sort_by_key(|&edge| (ends[edge].0.abs_diff(ends[edge].1), edge));
    for edge in order {
        let (from, to) = ends[edge];
        let width = labels[edge].as_ref().map_or(0., |(_, w, _)| *w);
        if from == to {
            let need = LOOP_W + 6. + width + 12.;
            // The last participant's loop has the room past it.
            if let Some(gap) = gaps.get_mut(from) {
                *gap = gap.max(need);
            }
            continue;
        }
        let (low, high) = (from.min(to), from.max(to));
        let need = width + 20.;
        let have: f32 = gaps[low..high].iter().sum();
        if have < need {
            let each = (need - have) / (high - low) as f32;
            for gap in &mut gaps[low..high] {
                *gap += each;
            }
        }
    }
    let mut centers = vec![sized.first().map_or(0., |s| s.w / 2.)];
    for gap in &gaps {
        let last = centers[centers.len() - 1];
        centers.push(last + gap);
    }

    let mut laid = Layout::empty();
    let head_height = sized.iter().map(|s| s.h).fold(0., f32::max);
    for (node, sized) in sized.iter().enumerate() {
        let rect = Rect::new(centers[node] - sized.w / 2., 0., sized.w, head_height);
        laid.nodes.push(NodeBox {
            node,
            rect,
            figure: sized.figure,
            tone: diagram.nodes()[node].tone,
        });
        node_texts(measure, node, sized, rect, &mut laid.texts);
    }

    let mut y = head_height + FIRST;
    for (edge, &(from, to)) in ends.iter().enumerate() {
        let style = diagram.edges()[edge].style();
        let label = labels[edge].as_ref();
        let (lines, label_w, label_h) = label
            .map(|(lines, w, h)| (lines.as_slice(), *w, *h))
            .unwrap_or((&[], 0., 0.));
        let x = centers[from];
        if from == to {
            let tall = LOOP_H.max(label_h);
            let top = y + (tall - LOOP_H) / 2.;
            let points = [
                Point::new(x, top),
                Point::new(x + LOOP_W, top),
                Point::new(x + LOOP_W, top + LOOP_H),
                Point::new(x + HEAD_LENGTH, top + LOOP_H),
            ];
            let rect = label.map(|_| {
                Rect::new(
                    x + LOOP_W + 6.,
                    top + LOOP_H / 2. - label_h / 2.,
                    label_w,
                    label_h,
                )
            });
            laid.edges.push(EdgePath {
                edge,
                from,
                to,
                segs: super::geometry::rounded(&points, 6.),
                head: Some(Head {
                    tip: Point::new(x, top + LOOP_H),
                    back: points[3],
                }),
                style,
                label: rect,
            });
            if let Some(rect) = rect {
                laid.texts.extend(edge_texts(measure, edge, lines, rect));
            }
            y += tall + ROW;
            continue;
        }
        let line = y + if label.is_some() { label_h + 2. } else { 0. };
        let target = centers[to];
        let toward = if target > x { 1. } else { -1. };
        let back = Point::new(target - toward * HEAD_LENGTH, line);
        // The words between the first two lifelines when they fit there, else over the
        // whole arrow.
        let next = if target > x { from + 1 } else { from - 1 };
        let first_gap = (centers[next] - x).abs();
        let middle = if first_gap >= label_w + 16. {
            (x + centers[next]) / 2.
        } else {
            (x + target) / 2.
        };
        let rect = label.map(|_| Rect::new(middle - label_w / 2., y, label_w, label_h));
        laid.edges.push(EdgePath {
            edge,
            from,
            to,
            segs: vec![super::Seg::Line(Point::new(x, line), back)],
            head: Some(Head {
                tip: Point::new(target, line),
                back,
            }),
            style,
            label: rect,
        });
        if let Some(rect) = rect {
            laid.texts.extend(edge_texts(measure, edge, lines, rect));
        }
        y = line + ROW;
    }
    let bottom = y - ROW + 12.;
    for &x in &centers {
        laid.guides.push(Guide {
            from: Point::new(x, head_height),
            to: Point::new(x, bottom.max(head_height + 24.)),
            kind: GuideKind::Lifeline,
        });
    }
    laid.fit()
}
