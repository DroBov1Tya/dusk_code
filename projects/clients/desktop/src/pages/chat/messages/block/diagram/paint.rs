//! The strokes of a diagram on one canvas: edges and their heads, the masks under edge
//! labels, guides, and the figures a box cannot draw (diamond, cylinder, dots).

use dusk_protocol::thread::diagram::{
    EdgeStyle,
    layout::{Figure, GuideKind, HEAD_HALF, Layout, NodeBox, Point as At, Seg},
};
use gpui_kit::*;

use super::Palette;

/// What the pointer lights: the edges touching the node under it, and their nodes.
#[derive(Clone, Default)]
pub struct Lit {
    pub node: Option<usize>,
    pub edges: Vec<usize>,
    pub nodes: Vec<usize>,
}

impl Lit {
    pub fn of(layout: &Layout, node: Option<usize>) -> Self {
        let Some(node) = node else {
            return Self::default();
        };
        let mut lit = Self {
            node: Some(node),
            edges: Vec::new(),
            nodes: vec![node],
        };
        for edge in &layout.edges {
            if edge.from == node || edge.to == node {
                lit.edges.push(edge.edge);
                lit.nodes.extend([edge.from, edge.to]);
            }
        }
        lit
    }

    /// Whether an edge is drawn quiet: something else is lit.
    pub fn dims_edge(&self, edge: usize) -> bool {
        self.node.is_some() && !self.edges.contains(&edge)
    }

    pub fn dims_node(&self, node: usize) -> bool {
        self.node.is_some() && !self.nodes.contains(&node)
    }
}

/// How far a part the pointer leaves out fades.
pub const DIM: f32 = 0.35;

/// The canvas under the boxes and the words.
pub fn canvas_layer(layout: Layout, palette: Palette, lit: Lit) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let at = |p: At| point(bounds.origin.x + px(p.x), bounds.origin.y + px(p.y));
            for guide in &layout.guides {
                let (color, width, dash) = match guide.kind {
                    GuideKind::Lifeline => (palette.hair, 1., Some([px(3.), px(4.)])),
                    GuideKind::Axis => (palette.edge.opacity(0.7), 1.5, None),
                    GuideKind::Drop => (palette.hair, 1., None),
                };
                stroke(
                    window,
                    &[Seg::Line(guide.from, guide.to)],
                    &at,
                    color,
                    width,
                    dash,
                );
            }
            for edge in &layout.edges {
                let lit_up = lit.edges.contains(&edge.edge);
                let mut color = match edge.style {
                    EdgeStyle::Strong => palette.accent,
                    _ => palette.edge,
                };
                if lit_up {
                    color = palette.accent;
                } else if lit.dims_edge(edge.edge) {
                    color = color.opacity(DIM);
                }
                let width = match (edge.style, lit_up) {
                    (EdgeStyle::Strong, _) | (_, true) => 1.75,
                    _ => 1.25,
                };
                let dash = (edge.style == EdgeStyle::Dashed).then_some([px(4.), px(3.)]);
                stroke(window, &edge.segs, &at, color, width, dash);
                if let Some(head) = edge.head {
                    let corners = head.corners(HEAD_HALF);
                    fill(window, &corners.map(at), color);
                }
                if let Some(label) = edge.label {
                    window.paint_quad(
                        gpui_kit::fill(
                            Bounds::new(
                                at(At::new(label.x, label.y)),
                                size(px(label.w), px(label.h)),
                            ),
                            palette.background,
                        )
                        .corner_radii(px(4.)),
                    );
                }
            }
            for node in &layout.nodes {
                let faded = lit.dims_node(node.node);
                figure(window, node, &at, &palette, faded);
            }
        },
    )
    .size_full()
}

/// The figures drawn here; boxes and pills are the caller's.
fn figure(
    window: &mut Window,
    node: &NodeBox,
    at: &dyn Fn(At) -> Point<Pixels>,
    palette: &Palette,
    faded: bool,
) {
    let (fill_color, border) = palette.tone(node.tone);
    let fade = |color: Hsla| if faded { color.opacity(DIM) } else { color };
    let r = node.rect;
    let center = r.center();
    match node.figure {
        Figure::Box | Figure::Pill => {}
        Figure::Diamond => {
            let corners = [
                At::new(center.x, r.y),
                At::new(r.right(), center.y),
                At::new(center.x, r.bottom()),
                At::new(r.x, center.y),
            ];
            fill(window, &corners.map(at), fade(palette.background));
            fill(window, &corners.map(at), fade(fill_color));
            let mut outline: Vec<Seg> = corners
                .windows(2)
                .map(|pair| Seg::Line(pair[0], pair[1]))
                .collect();
            outline.push(Seg::Line(corners[3], corners[0]));
            stroke(window, &outline, at, fade(border), 1., None);
        }
        Figure::Store => {
            let ry = 5.;
            let (top, bottom) = (r.y + ry, r.bottom() - ry);
            let rx = r.w / 2.;
            // The body: down the sides, round the bottom, back along the lid's far edge.
            let mut body = vec![Seg::Line(At::new(r.x, top), At::new(r.x, bottom))];
            body.extend(half_ellipse(center.x, bottom, rx, ry, true));
            body.push(Seg::Line(
                At::new(r.right(), bottom),
                At::new(r.right(), top),
            ));
            body.extend(
                half_ellipse(center.x, top, rx, ry, false)
                    .into_iter()
                    .rev()
                    .map(reverse),
            );
            let outline_points: Vec<Point<Pixels>> =
                dusk_protocol::thread::diagram::layout::polyline(&body)
                    .into_iter()
                    .map(at)
                    .collect();
            fill(window, &outline_points, fade(palette.background));
            fill(window, &outline_points, fade(fill_color));
            stroke(window, &body, at, fade(border), 1., None);
            // The lid's near edge.
            stroke(
                window,
                &half_ellipse(center.x, top, rx, ry, true),
                at,
                fade(border),
                1.,
                None,
            );
        }
        Figure::Dot => circle(window, center, r.w / 2., at, fade(palette.text)),
        Figure::Ring => {
            stroke(
                window,
                &ellipse(center.x, center.y, r.w / 2. - 0.75, r.w / 2. - 0.75),
                at,
                fade(palette.text),
                1.5,
                None,
            );
            circle(window, center, r.w / 2. - 4., at, fade(palette.text));
        }
        Figure::Event => {
            let color = match node.tone {
                Some(_) => border,
                None => palette.muted,
            };
            circle(window, center, r.w / 2. + 2., at, palette.background);
            circle(window, center, r.w / 2., at, fade(color));
        }
    }
}

fn reverse(seg: Seg) -> Seg {
    match seg {
        Seg::Line(a, b) => Seg::Line(b, a),
        Seg::Cubic(a, b, c, d) => Seg::Cubic(d, c, b, a),
    }
}

/// The cubic curves of a quarter ellipse are 0.55 of the radius from the corner.
const K: f32 = 0.5523;

/// Half an ellipse from its left end to its right, under its centre line or over it.
fn half_ellipse(cx: f32, cy: f32, rx: f32, ry: f32, under: bool) -> Vec<Seg> {
    let dy = if under { ry } else { -ry };
    vec![
        Seg::Cubic(
            At::new(cx - rx, cy),
            At::new(cx - rx, cy + dy * K),
            At::new(cx - rx * K, cy + dy),
            At::new(cx, cy + dy),
        ),
        Seg::Cubic(
            At::new(cx, cy + dy),
            At::new(cx + rx * K, cy + dy),
            At::new(cx + rx, cy + dy * K),
            At::new(cx + rx, cy),
        ),
    ]
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Seg> {
    let mut segs = half_ellipse(cx, cy, rx, ry, true);
    segs.extend(
        half_ellipse(cx, cy, rx, ry, false)
            .into_iter()
            .rev()
            .map(reverse),
    );
    segs
}

fn circle(window: &mut Window, center: At, r: f32, at: &dyn Fn(At) -> Point<Pixels>, color: Hsla) {
    let points: Vec<Point<Pixels>> =
        dusk_protocol::thread::diagram::layout::polyline(&ellipse(center.x, center.y, r, r))
            .into_iter()
            .map(at)
            .collect();
    fill(window, &points, color);
}

fn stroke(
    window: &mut Window,
    segs: &[Seg],
    at: &dyn Fn(At) -> Point<Pixels>,
    color: Hsla,
    width: f32,
    dash: Option<[Pixels; 2]>,
) {
    let Some(first) = segs.first() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        path = path.dash_array(&dash);
    }
    path.move_to(at(first.start()));
    for seg in segs {
        match *seg {
            Seg::Line(_, b) => path.line_to(at(b)),
            Seg::Cubic(_, b, c, d) => path.cubic_bezier_to(at(d), at(b), at(c)),
        }
    }
    // A path the tessellator refuses is left out: a line is not worth an error.
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn fill(window: &mut Window, points: &[Point<Pixels>], color: Hsla) {
    let mut path = PathBuilder::fill();
    path.add_polygon(points, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
