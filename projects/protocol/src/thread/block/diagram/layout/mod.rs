//! A diagram laid out: where each node, edge, group and text stands, in pixels, so every
//! client draws the same picture from the same data and decides nothing. The layout is a
//! pure function of the diagram, the measure of its texts and the width there is: the same
//! input always gives the same picture.
//!
//! Flow, architecture, tree and state are put in layers along their direction (`layered`);
//! a sequence in lanes, one a participant (`sequence`); a timeline along an axis
//! (`timeline`). When the picture is wider than the room, the layout tries the narrower
//! forms in turn: down instead of right, shorter lines, a tree as an outline, a timeline
//! standing; a picture still too wide is the narrowest one, and the client scrolls it.

mod geometry;
mod layered;
mod sequence;
mod text;
mod timeline;
mod tree;

pub use geometry::{Point, Rect, Seg, dashes, polyline};
pub use text::{Estimate, Measure, TextStyle, wrap};

use super::spec::{Diagram, DiagramKind, DiagramNode, DiagramTone, Direction, EdgeStyle, Shape};

/// The picture of a diagram. Drawn back to front: groups, guides, edges with the masks
/// of their labels, nodes, then every text.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub groups: Vec<GroupBox>,
    pub guides: Vec<Guide>,
    pub edges: Vec<EdgePath>,
    pub nodes: Vec<NodeBox>,
    pub texts: Vec<Text>,
}

/// The box around a group's nodes. Its name is one of the texts.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBox {
    pub group: usize,
    pub rect: Rect,
}

/// A line that is not an edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Guide {
    pub from: Point,
    pub to: Point,
    pub kind: GuideKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// A participant's line down the sequence: dashed and faint.
    Lifeline,
    /// A timeline's axis: solid.
    Axis,
    /// From an event's dot to its words: a hairline.
    Drop,
}

/// How a node is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Figure {
    /// A rounded box.
    Box,
    /// A box with round ends: a start or an end of a flow.
    Pill,
    /// A decision.
    Diamond,
    /// A cylinder: something that keeps data.
    Store,
    /// A filled dot: where a state machine starts.
    Dot,
    /// A dot in a ring: where it ends.
    Ring,
    /// A small dot on a timeline's axis.
    Event,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeBox {
    pub node: usize,
    pub rect: Rect,
    pub figure: Figure,
    pub tone: Option<DiagramTone>,
}

/// An edge's line, its arrowhead and the box its label stands in, which a client fills
/// with the background first so the line does not run through the words.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePath {
    pub edge: usize,
    pub from: usize,
    pub to: usize,
    pub segs: Vec<Seg>,
    pub head: Option<Head>,
    pub style: EdgeStyle,
    pub label: Option<Rect>,
}

/// A filled triangle whose point is `tip` and whose base is centred on `back`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Head {
    pub tip: Point,
    pub back: Point,
}

impl Head {
    /// The triangle's three corners, `half` pixels each side of the line at its base.
    pub fn corners(&self, half: f32) -> [Point; 3] {
        let (dx, dy) = (self.tip.x - self.back.x, self.tip.y - self.back.y);
        let length = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / length * half, dx / length * half);
        [
            self.tip,
            Point::new(self.back.x + nx, self.back.y + ny),
            Point::new(self.back.x - nx, self.back.y - ny),
        ]
    }
}

/// The length of an arrowhead and half its width.
pub const HEAD_LENGTH: f32 = 7.;
pub const HEAD_HALF: f32 = 3.5;

/// Where a text's `x` is: its left end, its middle, or its right end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// What a text belongs to, so a client can light it with its node or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Node(usize),
    Edge(usize),
    Group(usize),
}

/// One line of text. `y` is the top of its line box, `line_height` of its style tall;
/// the client centres the glyphs in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub anchor: Anchor,
    pub style: TextStyle,
    pub owner: Owner,
}

impl Text {
    /// The left end of the text.
    pub fn left(&self) -> f32 {
        match self.anchor {
            Anchor::Start => self.x,
            Anchor::Middle => self.x - self.width / 2.,
            Anchor::End => self.x - self.width,
        }
    }
}

/// Space left around the whole picture.
const MARGIN: f32 = 4.;

/// How long a line of a node's label may be, in the forms tried one after the other.
const WRAPS: [f32; 3] = [168., 128., 96.];

/// Lays `diagram` out to fit `width` pixels if it can.
pub fn layout(diagram: &Diagram, measure: &dyn Measure, width: f32) -> Layout {
    let mut tries: Vec<Layout> = Vec::new();
    let fits = |layout: &Layout| layout.width <= width;
    match diagram.kind() {
        DiagramKind::Sequence => {
            for wrap in WRAPS {
                let laid = sequence::layout(diagram, measure, wrap);
                if fits(&laid) {
                    return laid;
                }
                tries.push(laid);
            }
        }
        DiagramKind::Timeline => {
            let across = timeline::across(diagram, measure);
            if fits(&across) {
                return across;
            }
            return timeline::down(diagram, measure, width);
        }
        _ => {
            let mut directions = vec![diagram.direction()];
            if diagram.direction() == Direction::Right {
                directions.push(Direction::Down);
            }
            for direction in directions {
                for wrap in WRAPS {
                    let laid = layered::layout(diagram, measure, direction, wrap);
                    if fits(&laid) {
                        return laid;
                    }
                    tries.push(laid);
                }
            }
            if diagram.kind() == DiagramKind::Tree {
                for wrap in WRAPS {
                    let laid = tree::outline(diagram, measure, wrap);
                    if fits(&laid) {
                        return laid;
                    }
                    tries.push(laid);
                }
            }
        }
    }
    tries
        .into_iter()
        .reduce(|best, next| if next.width < best.width { next } else { best })
        .unwrap_or_else(|| sequence::layout(diagram, measure, WRAPS[0]))
}

/// A node's texts and the size of its figure.
struct Sized {
    w: f32,
    h: f32,
    figure: Figure,
    label: Vec<String>,
    note: Vec<String>,
}

const PAD_X: f32 = 12.;
const PAD_Y: f32 = 8.;
/// The gap between a node's label and its note.
const NOTE_GAP: f32 = 1.;

fn figure(shape: Shape) -> Figure {
    match shape {
        Shape::Box => Figure::Box,
        Shape::Pill => Figure::Pill,
        Shape::Diamond => Figure::Diamond,
        Shape::Store => Figure::Store,
        Shape::Start => Figure::Dot,
        Shape::End => Figure::Ring,
    }
}

/// How big a node is drawn, with its label and note wrapped to `wrap` pixels.
fn size(measure: &dyn Measure, node: &DiagramNode, wrap: f32) -> Sized {
    let figure = figure(node.shape());
    match figure {
        Figure::Dot => {
            return Sized {
                w: 14.,
                h: 14.,
                figure,
                label: Vec::new(),
                note: Vec::new(),
            };
        }
        Figure::Ring => {
            return Sized {
                w: 18.,
                h: 18.,
                figure,
                label: Vec::new(),
                note: Vec::new(),
            };
        }
        _ => {}
    }
    // A diamond grows twice as fast as its words: it keeps its lines short.
    let wrap = if figure == Figure::Diamond {
        wrap.min(120.)
    } else {
        wrap
    };
    let label = text::wrap(measure, node.label(), TextStyle::Label, wrap, 2);
    let note = node.note.as_ref().map_or_else(Vec::new, |note| {
        text::wrap(measure, note.as_str(), TextStyle::Note, wrap, 2)
    });
    let text_w = text::widest(measure, &label, TextStyle::Label).max(text::widest(
        measure,
        &note,
        TextStyle::Note,
    ));
    let text_h = text_height(&label, &note);
    let (w, h) = match figure {
        Figure::Diamond => {
            let h = text_h + 30.;
            let w = text_w / (1. - text_h / h) + 16.;
            (w.max(88.), h)
        }
        Figure::Pill => (text_w + 2. * PAD_X + 12., (text_h + 2. * PAD_Y).max(34.)),
        Figure::Store => (text_w + 2. * PAD_X, (text_h + 2. * PAD_Y).max(36.) + 8.),
        _ => (text_w + 2. * PAD_X, (text_h + 2. * PAD_Y).max(36.)),
    };
    Sized {
        // Widths in steps of 8 so nodes of about one size line up as one.
        w: (w.max(64.) / 8.).ceil() * 8.,
        h: (h / 2.).ceil() * 2.,
        figure,
        label,
        note,
    }
}

fn text_height(label: &[String], note: &[String]) -> f32 {
    let gap = if note.is_empty() { 0. } else { NOTE_GAP };
    label.len() as f32 * TextStyle::Label.line_height()
        + gap
        + note.len() as f32 * TextStyle::Note.line_height()
}

/// The lines of a node's label and note, centred in `rect`.
fn node_texts(
    measure: &dyn Measure,
    node: usize,
    sized: &Sized,
    rect: Rect,
    texts: &mut Vec<Text>,
) {
    let center = rect.center();
    // A cylinder's lid takes the top of its box.
    let lid = if sized.figure == Figure::Store {
        4.
    } else {
        0.
    };
    let mut y = center.y + lid - text_height(&sized.label, &sized.note) / 2.;
    let lines = sized
        .label
        .iter()
        .map(|line| (line, TextStyle::Label))
        .chain(sized.note.iter().map(|line| (line, TextStyle::Note)));
    for (at, (line, style)) in lines.enumerate() {
        if at == sized.label.len() && at > 0 {
            y += NOTE_GAP;
        }
        texts.push(Text {
            text: line.clone(),
            x: center.x,
            y,
            width: measure.width(line, style),
            anchor: Anchor::Middle,
            style,
            owner: Owner::Node(node),
        });
        y += style.line_height();
    }
}

impl Layout {
    fn empty() -> Self {
        Self {
            width: 0.,
            height: 0.,
            groups: Vec::new(),
            guides: Vec::new(),
            edges: Vec::new(),
            nodes: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// Moves everything so the picture starts `MARGIN` from the top left, and sets its
    /// size to what it covers.
    fn fit(mut self) -> Self {
        let mut boxes: Vec<Rect> = Vec::new();
        boxes.extend(self.groups.iter().map(|g| g.rect));
        boxes.extend(self.nodes.iter().map(|n| n.rect));
        boxes.extend(self.edges.iter().filter_map(|e| e.label));
        boxes.extend(
            self.texts
                .iter()
                .map(|t| Rect::new(t.left(), t.y, t.width, t.style.line_height())),
        );
        for guide in &self.guides {
            boxes.push(Rect::new(guide.from.x, guide.from.y, 0., 0.));
            boxes.push(Rect::new(guide.to.x, guide.to.y, 0., 0.));
        }
        for edge in &self.edges {
            for point in polyline(&edge.segs) {
                boxes.push(Rect::new(point.x, point.y, 0., 0.));
            }
        }
        let Some(first) = boxes.first().copied() else {
            return self;
        };
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
        for b in &boxes {
            x0 = x0.min(b.x);
            y0 = y0.min(b.y);
            x1 = x1.max(b.right());
            y1 = y1.max(b.bottom());
        }
        let (dx, dy) = (MARGIN - x0, MARGIN - y0);
        for group in &mut self.groups {
            group.rect = group.rect.moved(dx, dy);
        }
        for node in &mut self.nodes {
            node.rect = node.rect.moved(dx, dy);
        }
        for guide in &mut self.guides {
            guide.from = Point::new(guide.from.x + dx, guide.from.y + dy);
            guide.to = Point::new(guide.to.x + dx, guide.to.y + dy);
        }
        for edge in &mut self.edges {
            for seg in &mut edge.segs {
                *seg = seg.moved(dx, dy);
            }
            if let Some(head) = &mut edge.head {
                head.tip = Point::new(head.tip.x + dx, head.tip.y + dy);
                head.back = Point::new(head.back.x + dx, head.back.y + dy);
            }
            edge.label = edge.label.map(|label| label.moved(dx, dy));
        }
        for text in &mut self.texts {
            text.x += dx;
            text.y += dy;
        }
        self.width = (x1 - x0 + 2. * MARGIN).ceil();
        self.height = (y1 - y0 + 2. * MARGIN).ceil();
        self
    }
}

/// The words of an edge's label in lines, and the box they need with a little room
/// around them.
fn edge_label(measure: &dyn Measure, label: &str, wrap: f32) -> (Vec<String>, f32, f32) {
    let lines = text::wrap(measure, label, TextStyle::Edge, wrap, 2);
    let w = text::widest(measure, &lines, TextStyle::Edge) + 8.;
    let h = lines.len() as f32 * TextStyle::Edge.line_height() + 2.;
    (lines, w, h)
}

/// The texts of an edge's label, centred in `rect`.
fn edge_texts(measure: &dyn Measure, edge: usize, lines: &[String], rect: Rect) -> Vec<Text> {
    let line = TextStyle::Edge.line_height();
    let top = rect.center().y - lines.len() as f32 * line / 2.;
    lines
        .iter()
        .enumerate()
        .map(|(at, text)| Text {
            text: text.clone(),
            x: rect.center().x,
            y: top + at as f32 * line,
            width: measure.width(text, TextStyle::Edge),
            anchor: Anchor::Middle,
            style: TextStyle::Edge,
            owner: Owner::Edge(edge),
        })
        .collect()
}

pub use layered::reading_order;

#[cfg(test)]
mod tests;
