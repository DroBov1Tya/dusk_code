//! What both layers draw with: a box at the place the layout gave it, the edges on one
//! canvas under the boxes, and the mark of a member or a task.

use dusk_protocol::thread::{
    TaskStatus, Work,
    team_layout::{Edge, EdgeTone, Ends, Lit, MemberState, Rect, Related},
};
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::activity,
    ui::{Type, widgets},
};

/// How far a node the selection leaves out fades.
const DIM: f32 = 0.38;

/// A box at its place in a layer.
pub fn at(rect: Rect) -> Div {
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
}

/// A layer of `width` by `height`: the edges under, the boxes over.
pub fn layer(width: f32, height: f32, edges: Div, boxes: Vec<AnyElement>) -> Div {
    div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .child(edges)
        .children(boxes)
}

/// A node's face as the selection leaves it: an outline when picked or related, faded
/// when left out.
pub fn lit(node: Stateful<Div>, lit: Option<Lit>, cx: &App) -> Stateful<Div> {
    let accent = cx.theme().primary;
    match lit {
        Some(Lit::Picked) => node.border_color(accent),
        Some(Lit::Related) => node.border_color(accent.opacity(0.45)),
        Some(Lit::Dim) => node.opacity(DIM),
        None => node,
    }
}

/// The colour of an edge of `tone`.
fn tone_color(tone: EdgeTone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        EdgeTone::Line => theme.foreground.opacity(0.30),
        EdgeTone::Faint => theme.foreground.opacity(0.11),
        EdgeTone::Accent => theme.primary,
        EdgeTone::Warning => theme.warning.opacity(0.9),
        EdgeTone::Danger => theme.danger.opacity(0.65),
    }
}

/// Every edge of a layer on one canvas; quiet ones first, so a live one crosses over.
/// With a selection, the edges it lights are thicker and the rest fade.
pub fn edges(mut edges: Vec<Edge>, related: Option<&Related>, cx: &App) -> Div {
    edges.sort_by_key(|edge| edge.tone != EdgeTone::Faint);
    let accent = cx.theme().primary;
    let drawn: Vec<(Edge, Hsla, f32)> = edges
        .into_iter()
        .map(|edge| {
            let mut color = tone_color(edge.tone, cx);
            let shine = related.map(|related| related.edge(&edge.ends));
            if shine == Some(true) && matches!(edge.ends, Ends::Tasks(..)) && !edge.dashed {
                color = accent;
            }
            if shine == Some(false) {
                color = color.opacity(DIM);
            }
            let width = if shine == Some(true) { 1.6 } else { 1.25 };
            (edge, color, width)
        })
        .collect();
    div().absolute().top_0().left_0().size_full().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let place = |p: dusk_protocol::thread::team_layout::Point| {
                    point(bounds.origin.x + px(p.x), bounds.origin.y + px(p.y))
                };
                for (edge, color, width) in &drawn {
                    let mut path = PathBuilder::stroke(px(*width));
                    if edge.dashed {
                        let dash = match edge.ends {
                            Ends::Lead(_) => [px(2.), px(4.)],
                            _ => [px(5.), px(4.)],
                        };
                        path = path.dash_array(&dash);
                    }
                    let [from, a, b, to] = edge.curve;
                    path.move_to(place(from));
                    path.cubic_bezier_to(place(to), place(a), place(b));
                    // A path the tessellator refuses is left out: an edge is not worth
                    // an error.
                    if let Ok(path) = path.build() {
                        window.paint_path(path, *color);
                    }
                    if let Some([left, tip, right]) = edge.arrow {
                        let mut head = PathBuilder::stroke(px(*width));
                        head.move_to(place(left));
                        head.line_to(place(tip));
                        head.line_to(place(right));
                        if let Ok(head) = head.build() {
                            window.paint_path(head, *color);
                        }
                    }
                }
            },
        )
        .size_full(),
    )
}

/// A task status's glyph in its tone, in the slot a moving mark takes.
pub fn glyph(status: TaskStatus, cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(20.))
        .flex()
        .justify_center()
        .small()
        .text_color(widgets::tone(status.tone(), cx))
        .child(status.glyph())
}

/// A member's mark: the working row's moving one while it thinks, runs a step, writes
/// or waits for the user; its task's glyph otherwise.
pub fn member_mark(state: &MemberState, cx: &App) -> Div {
    match state.work_kind() {
        Some(kind) => activity::mark(
            &Work {
                kind,
                label: String::new(),
                stalled: false,
            },
            cx,
        ),
        None => glyph(state.status(), cx),
    }
}
