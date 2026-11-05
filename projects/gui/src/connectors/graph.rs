//! The agents as a graph: each parent left of its children, joined by curves, and the
//! agents one may ask elsewhere in the tree joined by dashed curves. The layout is
//! plain data, so it is tested without a window.

use std::collections::HashMap;

use dusk_protocol::{
    catalog::{AgentNode, AgentTree},
    client::Switch as CoreSwitch,
};
use gpui_kit::{
    component::{ActiveTheme, Sizable, switch::Switch},
    prelude::FluentBuilder,
    *,
};

use crate::{widgets, workspace::Workspace};

const NODE_W: f32 = 200.;
const NODE_H: f32 = 56.;
const COLUMN: f32 = 260.;
const ROW: f32 = 76.;
const PAD: f32 = 16.;

/// A node placed on the graph.
struct Placed<'a> {
    node: &'a AgentNode,
    x: f32,
    y: f32,
    /// The root is the chat itself, which is always on.
    root: bool,
}

struct Layout<'a> {
    nodes: Vec<Placed<'a>>,
    /// Parent to child, by index into `nodes`.
    children: Vec<(usize, usize)>,
    /// Asker to asked, for links to agents drawn elsewhere.
    links: Vec<(usize, usize)>,
    width: f32,
    height: f32,
}

fn layout(tree: &AgentTree) -> Layout<'_> {
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    let mut next_row = 0.;
    place(&tree.root, 0, &mut next_row, &mut nodes, &mut children);
    nodes[0].root = true;
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(at, placed)| (placed.node.name.as_str(), at))
        .collect();
    let links = nodes
        .iter()
        .enumerate()
        .flat_map(|(from, placed)| {
            let index = &index;
            placed
                .node
                .links
                .iter()
                .filter_map(move |name| index.get(name.as_str()).map(|&to| (from, to)))
        })
        .collect();
    let width = nodes.iter().map(|n| n.x).fold(0., f32::max) + NODE_W + PAD * 2.;
    let height = nodes.iter().map(|n| n.y).fold(0., f32::max) + NODE_H + PAD * 2.;
    Layout {
        nodes,
        children,
        links,
        width,
        height,
    }
}

/// Places `node` and its subtree; a leaf takes the next row, a parent sits midway
/// between its first and last child. Returns the node's index.
fn place<'a>(
    node: &'a AgentNode,
    depth: usize,
    next_row: &mut f32,
    nodes: &mut Vec<Placed<'a>>,
    edges: &mut Vec<(usize, usize)>,
) -> usize {
    let at = nodes.len();
    nodes.push(Placed {
        node,
        x: PAD + depth as f32 * COLUMN,
        y: 0.,
        root: false,
    });
    let kids: Vec<usize> = node
        .children
        .iter()
        .map(|child| place(child, depth + 1, next_row, nodes, edges))
        .collect();
    nodes[at].y = match (kids.first(), kids.last()) {
        (Some(&first), Some(&last)) => (nodes[first].y + nodes[last].y) / 2.,
        _ => {
            let y = PAD + *next_row * ROW;
            *next_row += 1.;
            y
        }
    };
    edges.extend(kids.into_iter().map(|kid| (at, kid)));
    at
}

pub fn render(tree: &AgentTree, cx: &mut Context<Workspace>) -> Div {
    let layout = layout(tree);
    let theme = cx.theme();
    let (border, accent, muted, surface, danger) =
        (theme.border, theme.primary, theme.muted_foreground, theme.muted, theme.danger);
    // Edges run from the right side of the parent to the left side of the child.
    let ends = |from: &Placed, to: &Placed| {
        (
            (from.x + NODE_W, from.y + NODE_H / 2.),
            (to.x, to.y + NODE_H / 2.),
        )
    };
    let solid: Vec<_> = layout
        .children
        .iter()
        .map(|&(from, to)| ends(&layout.nodes[from], &layout.nodes[to]))
        .collect();
    let dashed: Vec<_> = layout
        .links
        .iter()
        .map(|&(from, to)| ends(&layout.nodes[from], &layout.nodes[to]))
        .collect();
    let edges = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let origin = bounds.origin;
            let at = |(x, y): (f32, f32)| point(origin.x + px(x), origin.y + px(y));
            let curve = |path: &mut PathBuilder, (a, b): ((f32, f32), (f32, f32))| {
                let bend = ((b.0 - a.0).abs() / 2.).max(40.);
                path.move_to(at(a));
                path.cubic_bezier_to(at(b), at((a.0 + bend, a.1)), at((b.0 - bend, b.1)));
            };
            let mut lines = PathBuilder::stroke(px(1.5));
            for edge in &solid {
                curve(&mut lines, *edge);
            }
            if let Ok(path) = lines.build() {
                window.paint_path(path, border);
            }
            let mut links = PathBuilder::stroke(px(1.2)).dash_array(&[px(5.), px(4.)]);
            for edge in &dashed {
                curve(&mut links, *edge);
            }
            if let Ok(path) = links.build() {
                window.paint_path(path, accent.opacity(0.7));
            }
        },
    )
    .absolute()
    .size_full();
    let mut graph = div()
        .relative()
        .w(px(layout.width))
        .h(px(layout.height))
        .child(edges);
    for placed in &layout.nodes {
        let node = placed.node;
        let name = node.name.clone();
        graph = graph.child(
            div()
                .absolute()
                .left(px(placed.x))
                .top(px(placed.y))
                .w(px(NODE_W))
                .h(px(NODE_H))
                .px_3()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(if placed.root { accent } else { border })
                .bg(surface)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .truncate()
                                .child(node.description.clone()),
                        ),
                )
                .when(!placed.root, |row| {
                    row.child(
                        Switch::new(SharedString::from(format!("agent-{name}")))
                            .checked(node.enabled)
                            .xsmall()
                            .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                ws.switch(CoreSwitch::Agent(name.clone()), *on, cx);
                            })),
                    )
                }),
        );
    }
    let mut page = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_4()
                .text_xs()
                .text_color(muted)
                .child("── asks as its child")
                .child(div().text_color(accent).child("- - may also ask")),
        )
        .child(div().id("agents-graph").overflow_x_scroll().child(graph));
    for problem in tree.problems.iter().chain(&tree.skipped) {
        page = page.child(
            div()
                .text_xs()
                .text_color(danger)
                .child(format!("{}: {}", problem.node, problem.reason)),
        );
    }
    page.child(widgets::muted("The chat at the left asks the agents to its right.", cx))
}

#[cfg(test)]
mod tests {
    use super::{AgentNode, AgentTree, layout};

    fn node(name: &str, children: Vec<AgentNode>, links: &[&str]) -> AgentNode {
        AgentNode {
            name: name.into(),
            description: String::new(),
            version: None,
            enabled: true,
            links: links.iter().map(|link| link.to_string()).collect(),
            children,
        }
    }

    #[test]
    fn parents_sit_left_of_and_between_their_children() {
        let tree = AgentTree {
            root: node(
                "chat",
                vec![
                    node("plan", vec![node("a", vec![], &[]), node("b", vec![], &["review"])], &[]),
                    node("review", vec![], &[]),
                ],
                &[],
            ),
            problems: vec![],
            skipped: vec![],
        };
        let laid = layout(&tree);
        let y = |name: &str| laid.nodes.iter().find(|n| n.node.name == name).unwrap().y;
        let x = |name: &str| laid.nodes.iter().find(|n| n.node.name == name).unwrap().x;
        assert!(x("chat") < x("plan") && x("plan") < x("a"));
        assert_eq!(y("plan"), (y("a") + y("b")) / 2.);
        assert_eq!(laid.children.len(), 4);
        assert_eq!(laid.links.len(), 1);
        assert!(laid.nodes[0].root);
    }
}
