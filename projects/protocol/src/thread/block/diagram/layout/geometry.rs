//! Points, boxes and the pieces of a drawn line, in pixels from the diagram's top left.

/// A point of the diagram.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn lerp(self, to: Point, t: f32) -> Point {
        Point::new(self.x + (to.x - self.x) * t, self.y + (to.y - self.y) * t)
    }

    fn distance(self, to: Point) -> f32 {
        ((to.x - self.x).powi(2) + (to.y - self.y).powi(2)).sqrt()
    }
}

/// A box of the diagram.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2., self.y + self.h / 2.)
    }

    /// Whether the two boxes share any area, not only an edge.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right() - 0.01
            && other.x < self.right() - 0.01
            && self.y < other.bottom() - 0.01
            && other.y < self.bottom() - 0.01
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x - 0.01
            && point.x <= self.right() + 0.01
            && point.y >= self.y - 0.01
            && point.y <= self.bottom() + 0.01
    }

    pub(super) fn moved(self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// One piece of a drawn line: straight, or a cubic Bézier curve with two control points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line(Point, Point),
    Cubic(Point, Point, Point, Point),
}

impl Seg {
    pub fn start(&self) -> Point {
        match *self {
            Seg::Line(a, _) | Seg::Cubic(a, _, _, _) => a,
        }
    }

    pub fn end(&self) -> Point {
        match *self {
            Seg::Line(_, b) | Seg::Cubic(_, _, _, b) => b,
        }
    }

    pub(super) fn moved(self, dx: f32, dy: f32) -> Seg {
        let m = |p: Point| Point::new(p.x + dx, p.y + dy);
        match self {
            Seg::Line(a, b) => Seg::Line(m(a), m(b)),
            Seg::Cubic(a, b, c, d) => Seg::Cubic(m(a), m(b), m(c), m(d)),
        }
    }

    /// The point at `t` of the piece, from 0 at its start to 1 at its end.
    pub fn at(&self, t: f32) -> Point {
        match *self {
            Seg::Line(a, b) => a.lerp(b, t),
            Seg::Cubic(a, b, c, d) => {
                let (ab, bc, cd) = (a.lerp(b, t), b.lerp(c, t), c.lerp(d, t));
                ab.lerp(bc, t).lerp(bc.lerp(cd, t), t)
            }
        }
    }
}

/// The points of a line made of `segs`, curves cut into short straight steps: for a
/// client that draws only straight strokes, or dashes.
pub fn polyline(segs: &[Seg]) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    for seg in segs {
        if points.is_empty() {
            points.push(seg.start());
        }
        match seg {
            Seg::Line(_, b) => points.push(*b),
            Seg::Cubic(..) => {
                const STEPS: usize = 12;
                points.extend((1..=STEPS).map(|step| seg.at(step as f32 / STEPS as f32)));
            }
        }
    }
    points
}

/// The dashes of a polyline, `on` pixels drawn and `off` left out, each a short polyline.
pub fn dashes(points: &[Point], on: f32, off: f32) -> Vec<Vec<Point>> {
    let mut dashes = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    // How far into the current dash or gap the walk is, and which of the two it is in.
    let (mut into, mut drawing) = (0., true);
    for pair in points.windows(2) {
        let (mut from, to) = (pair[0], pair[1]);
        let mut left = from.distance(to);
        while left > 0.001 {
            let span = if drawing { on } else { off };
            let step = (span - into).min(left);
            let next = from.lerp(to, step / left);
            if drawing {
                if current.is_empty() {
                    current.push(from);
                }
                current.push(next);
            }
            into += step;
            left -= step;
            from = next;
            if into >= span - 0.001 {
                if drawing {
                    dashes.push(std::mem::take(&mut current));
                }
                drawing = !drawing;
                into = 0.;
            }
        }
    }
    if current.len() > 1 {
        dashes.push(current);
    }
    dashes
}

/// A polyline with each corner rounded by up to `radius`, as pieces to draw.
pub(super) fn rounded(points: &[Point], radius: f32) -> Vec<Seg> {
    let points: Vec<Point> = points
        .iter()
        .enumerate()
        .filter(|&(at, p)| at == 0 || p.distance(points[at - 1]) > 0.01)
        .map(|(_, p)| *p)
        .collect();
    if points.len() < 2 {
        return Vec::new();
    }
    let mut segs = Vec::new();
    let mut from = points[0];
    for at in 1..points.len() - 1 {
        let (corner, next) = (points[at], points[at + 1]);
        let (before, after) = (from.distance(corner), corner.distance(next));
        let r = radius.min(before / 2.).min(after / 2.);
        let enter = corner.lerp(from, r / before.max(0.001));
        let leave = corner.lerp(next, r / after.max(0.001));
        segs.push(Seg::Line(from, enter));
        if r > 0.01 {
            // A quarter circle is close to a cubic with its controls 0.55 of the way in.
            let k = 0.552;
            segs.push(Seg::Cubic(
                enter,
                enter.lerp(corner, k),
                leave.lerp(corner, k),
                leave,
            ));
        }
        from = leave;
    }
    segs.push(Seg::Line(from, points[points.len() - 1]));
    segs.retain(|seg| seg.start().distance(seg.end()) > 0.01 || matches!(seg, Seg::Cubic(..)));
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_cover_the_line_in_steps() {
        let line = [
            Point::new(0., 0.),
            Point::new(10., 0.),
            Point::new(10., 10.),
        ];
        let dashes = dashes(&line, 4., 3.);
        // 20 px of 4 on and 3 off: dashes start at 0, 7 and 14.
        assert_eq!(dashes.len(), 3);
        assert_eq!(dashes[1][0], Point::new(7., 0.));
        assert_eq!(dashes[1].last().copied(), Some(Point::new(10., 1.)));
    }

    #[test]
    fn a_rounded_corner_keeps_the_ends() {
        let segs = rounded(
            &[
                Point::new(0., 0.),
                Point::new(0., 20.),
                Point::new(30., 20.),
            ],
            6.,
        );
        assert_eq!(segs.first().map(Seg::start), Some(Point::new(0., 0.)));
        assert_eq!(segs.last().map(Seg::end), Some(Point::new(30., 20.)));
        assert!(segs.iter().any(|seg| matches!(seg, Seg::Cubic(..))));
    }
}
