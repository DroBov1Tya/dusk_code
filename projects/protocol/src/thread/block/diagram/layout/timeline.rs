//! A timeline: events in order along an axis, evenly spaced. Across the page the words
//! take turns above and below the axis so neighbours do not collide, each tied to its dot
//! by a hairline; in a narrow column the axis stands and the words read beside it.

use super::geometry::{Point, Rect};
use super::text::{widest, wrap};
use super::{Anchor, Figure, Guide, GuideKind, Layout, Measure, NodeBox, Owner, Text, TextStyle};
use crate::thread::block::diagram::spec::{Diagram, DiagramTone};

/// How long a line of an event's words may be.
const WRAP: f32 = 150.;
/// Least space between two events along the axis.
const LEAST: f32 = 64.;
/// Space from the axis to the words.
const DROP: f32 = 16.;

/// One event's words: its date, label and note, each a style and its lines.
struct Words {
    when: Vec<String>,
    label: Vec<String>,
    note: Vec<String>,
    width: f32,
    height: f32,
}

fn words(measure: &dyn Measure, diagram: &Diagram, wrap_at: f32) -> Vec<Words> {
    diagram
        .nodes()
        .iter()
        .map(|node| {
            let when = node
                .when
                .as_ref()
                .map_or_else(Vec::new, |when| vec![when.as_str().to_owned()]);
            let label = wrap(measure, node.label(), TextStyle::Label, wrap_at, 2);
            let note = node.note.as_ref().map_or_else(Vec::new, |note| {
                wrap(measure, note.as_str(), TextStyle::Note, wrap_at, 2)
            });
            let width = widest(measure, &when, TextStyle::Axis)
                .max(widest(measure, &label, TextStyle::Label))
                .max(widest(measure, &note, TextStyle::Note));
            let height = when.len() as f32 * TextStyle::Axis.line_height()
                + label.len() as f32 * TextStyle::Label.line_height()
                + note.len() as f32 * TextStyle::Note.line_height();
            Words {
                when,
                label,
                note,
                width,
                height,
            }
        })
        .collect()
}

fn dot(diagram: &Diagram, node: usize, at: Point) -> NodeBox {
    let tone = diagram.nodes()[node].tone;
    let r = if tone == Some(DiagramTone::Muted) || tone.is_none() {
        4.
    } else {
        5.5
    };
    NodeBox {
        node,
        rect: Rect::new(at.x - r, at.y - r, 2. * r, 2. * r),
        figure: Figure::Event,
        tone,
    }
}

/// Lines of text from `top` down, each anchored at `x`.
fn stack(
    measure: &dyn Measure,
    lines: &[(&[String], TextStyle)],
    node: usize,
    x: f32,
    anchor: Anchor,
    mut top: f32,
    texts: &mut Vec<Text>,
) {
    for &(lines, style) in lines {
        for line in lines {
            texts.push(Text {
                text: line.clone(),
                x,
                y: top,
                width: measure.width(line, style),
                anchor,
                style,
                owner: Owner::Node(node),
            });
            top += style.line_height();
        }
    }
}

/// The axis across the page.
pub(super) fn across(diagram: &Diagram, measure: &dyn Measure) -> Layout {
    let words = words(measure, diagram, WRAP);
    let count = words.len();
    let mut spacing = LEAST;
    for at in 0..count.saturating_sub(2) {
        let pair = (words[at].width + words[at + 2].width) / 2. + 16.;
        spacing = spacing.max(pair / 2.);
    }
    let above = words.iter().step_by(2).map(|w| w.height).fold(0., f32::max);
    let axis = above + DROP;
    let mut laid = Layout::empty();
    for (node, words) in words.iter().enumerate() {
        let x = node as f32 * spacing;
        laid.nodes.push(dot(diagram, node, Point::new(x, axis)));
        let (top, lines): (f32, Vec<(&[String], TextStyle)>) = if node % 2 == 0 {
            (
                axis - DROP - words.height,
                vec![
                    (&words.label, TextStyle::Label),
                    (&words.note, TextStyle::Note),
                    (&words.when, TextStyle::Axis),
                ],
            )
        } else {
            (
                axis + DROP,
                vec![
                    (&words.when, TextStyle::Axis),
                    (&words.label, TextStyle::Label),
                    (&words.note, TextStyle::Note),
                ],
            )
        };
        stack(
            measure,
            &lines,
            node,
            x,
            Anchor::Middle,
            top,
            &mut laid.texts,
        );
        let (from, to) = if node % 2 == 0 {
            (axis - 6., axis - DROP + 3.)
        } else {
            (axis + 6., axis + DROP - 3.)
        };
        laid.guides.push(Guide {
            from: Point::new(x, from),
            to: Point::new(x, to),
            kind: GuideKind::Drop,
        });
    }
    let last = (count.saturating_sub(1)) as f32 * spacing;
    laid.guides.insert(
        0,
        Guide {
            from: Point::new(-20., axis),
            to: Point::new(last + 20., axis),
            kind: GuideKind::Axis,
        },
    );
    laid.fit()
}

/// The axis standing, dates on its left and words on its right, in `width` pixels.
pub(super) fn down(diagram: &Diagram, measure: &dyn Measure, width: f32) -> Layout {
    let dates: Vec<f32> = diagram
        .nodes()
        .iter()
        .map(|node| {
            node.when
                .as_ref()
                .map_or(0., |when| measure.width(when.as_str(), TextStyle::Axis))
        })
        .collect();
    let date_room = dates.iter().copied().fold(0., f32::max);
    let axis = if date_room > 0. { date_room + 14. } else { 8. };
    let left = axis + DROP;
    let wrap_at = (width - left - 12.).clamp(120., 320.);
    let words = words(measure, diagram, wrap_at);
    let mut laid = Layout::empty();
    let mut top = 0.;
    let mut first_last = (0., 0.);
    for (node, words) in words.iter().enumerate() {
        let middle = top + TextStyle::Label.line_height() / 2.;
        laid.nodes
            .push(dot(diagram, node, Point::new(axis, middle)));
        stack(
            measure,
            &[(&words.when, TextStyle::Axis)],
            node,
            axis - 14.,
            Anchor::End,
            middle - TextStyle::Axis.line_height() / 2.,
            &mut laid.texts,
        );
        stack(
            measure,
            &[
                (&words.label, TextStyle::Label),
                (&words.note, TextStyle::Note),
            ],
            node,
            left,
            Anchor::Start,
            top,
            &mut laid.texts,
        );
        if node == 0 {
            first_last.0 = middle;
        }
        first_last.1 = middle;
        let own = words.height - words.when.len() as f32 * TextStyle::Axis.line_height();
        top += own.max(TextStyle::Label.line_height()) + 16.;
    }
    laid.guides.insert(
        0,
        Guide {
            from: Point::new(axis, first_last.0),
            to: Point::new(axis, first_last.1),
            kind: GuideKind::Axis,
        },
    );
    laid.fit()
}
