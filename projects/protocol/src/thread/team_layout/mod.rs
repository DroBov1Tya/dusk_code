//! The Team page laid out: where each node of its two layers stands and how its edges
//! run, in pixels, so the desktop and the browser draw the same picture and decide
//! nothing. **People** puts the lead on the left and its members in a column to the
//! right, with an edge for each task given or handed in and a dashed arc for who waits on
//! whom; **Plan** puts the tasks in waves by how deep they depend, so a task stands right
//! of everything it needs. Both answer one selection (`Related`).
//!
//! Every function here is pure: the team, the plan and the members' own states in, boxes
//! and words out. Every word that came from the lead, a member or a program is model
//! text: a client draws it as plain text.

mod member;
mod people;
mod plan;
mod select;

pub use member::Doing;
pub use people::{FoldChip, MemberNode, People, people};
pub use plan::{FOLD_PAST, Folded, Plan, TaskNode, WaveHead, plan, task_word, waves};
pub use select::{Lit, Pick, Related};

/// A point of a layer, in pixels from its top left corner.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A box of a layer, in pixels from its top left corner.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn middle_y(&self) -> f32 {
        self.y + self.h / 2.
    }

    fn right(&self) -> f32 {
        self.x + self.w
    }
}

/// How an edge is coloured; a client maps each to its theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeTone {
    /// A task given and held.
    Line,
    /// Done: the edge goes quiet.
    Faint,
    /// Handed in.
    Accent,
    /// Blocked, or waits on another.
    Warning,
    Danger,
}

/// What an edge joins, so a selection knows whether it lights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ends {
    /// The lead and the member of this thread.
    Lead(String),
    /// A member that waits, and the member whose task it waits on.
    Members(String, String),
    /// The task needed, and the task that needs it, by their ids.
    Tasks(String, String),
}

/// An edge: one cubic curve, how it looks, and its arrow head, if any, as the three
/// points of a chevron.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub ends: Ends,
    /// From, the two control points, to.
    pub curve: [Point; 4],
    pub tone: EdgeTone,
    pub dashed: bool,
    pub arrow: Option<[Point; 3]>,
}

/// Where an edge's words stand and which way they lean.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeLabel {
    pub text: String,
    pub rect: Rect,
    pub tone: EdgeTone,
    /// Right-aligned against the node it leads to, else left-aligned after the edge.
    pub right: bool,
    pub ends: Ends,
}

/// Which way an edge's arrow points.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrow {
    None,
    /// At the end it goes to.
    Out,
    /// At the end it comes from: a member hands back to the lead.
    In,
}

impl Edge {
    fn new(ends: Ends, curve: [Point; 4], tone: EdgeTone, dashed: bool, arrow: Arrow) -> Self {
        let arrow = match arrow {
            Arrow::None => None,
            Arrow::Out => Some(chevron(at(&curve, 1.), at(&curve, 0.9))),
            Arrow::In => Some(chevron(at(&curve, 0.), at(&curve, 0.1))),
        };
        Self {
            ends,
            curve,
            tone,
            dashed,
            arrow,
        }
    }
}

/// The point of a cubic curve at `t`, from 0 to 1.
fn at(curve: &[Point; 4], t: f32) -> Point {
    let u = 1. - t;
    let (a, b, c, d) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
    Point {
        x: a * curve[0].x + b * curve[1].x + c * curve[2].x + d * curve[3].x,
        y: a * curve[0].y + b * curve[1].y + c * curve[2].y + d * curve[3].y,
    }
}

/// An arrow head at `tip`, pointing away from `from`.
fn chevron(tip: Point, from: Point) -> [Point; 3] {
    const LENGTH: f32 = 6.;
    const SPREAD: f32 = 0.5;
    let angle = (tip.y - from.y).atan2(tip.x - from.x);
    let side = |turn: f32| Point {
        x: tip.x - LENGTH * (angle + turn).cos(),
        y: tip.y - LENGTH * (angle + turn).sin(),
    };
    [side(-SPREAD), tip, side(SPREAD)]
}

/// From the right side of `from` to the left side of `to`, leaving and arriving level.
fn across(from: Rect, to: Rect) -> [Point; 4] {
    let (x0, x1) = (from.right() + 2., to.x - 2.);
    let pull = ((x1 - x0) * 0.5).max(12.);
    [
        Point {
            x: x0,
            y: from.middle_y(),
        },
        Point {
            x: x0 + pull,
            y: from.middle_y(),
        },
        Point {
            x: x1 - pull,
            y: to.middle_y(),
        },
        Point {
            x: x1,
            y: to.middle_y(),
        },
    ]
}

/// From the right side of `from` out and back into the right side of `to`, `reach` wide.
fn arc(from: Rect, to: Rect, reach: f32) -> [Point; 4] {
    let (ay, by) = (from.middle_y(), to.middle_y());
    let down = if by > ay { 1. } else { -1. };
    let x0 = from.right().max(to.right()) + 3.;
    let (ya, yb) = (ay + down * 8., by - down * 8.);
    let out = x0 + reach * 1.33;
    [
        Point { x: x0, y: ya },
        Point { x: out, y: ya },
        Point { x: out, y: yb },
        Point { x: x0, y: yb },
    ]
}
