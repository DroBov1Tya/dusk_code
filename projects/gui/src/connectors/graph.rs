//! The agents, one level at a time: the agent in focus on the left, its own children on
//! the right joined by curves. A child with children of its own is entered, with a
//! breadcrumb back. Links to agents elsewhere in the tree are chips on the card, both
//! ways ("asks", "asked by"), never edges across the picture: crossing edges are what
//! made the whole tree at once unreadable.

use std::collections::BTreeMap;

use dusk_protocol::{
    catalog::{AgentNode, AgentTree},
    client::Switch as CoreSwitch,
};
use gpui_kit::{
    component::{ActiveTheme, IconName, Sizable, switch::Switch},
    *,
};

use crate::{modal::detail::Detail, widgets, workspace::Workspace};

const CARD_W: f32 = 300.;
const CARD_H: f32 = 88.;
const GAP: f32 = 12.;
const FOCUS_W: f32 = 240.;
const BRIDGE: f32 = 96.;

pub fn render(ws: &Workspace, tree: &AgentTree, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, border, accent, danger, surface) = (
        theme.muted_foreground,
        theme.border,
        theme.primary,
        theme.danger,
        theme.muted,
    );
    let paths = paths(&tree.root);
    let asked_by = asked_by(&tree.root);
    let (focus, trail) = focus(&tree.root, &ws.agent_path);
    // Breadcrumb: Agents › a › b, each a way back.
    let mut crumbs = div().flex().flex_row().items_center().gap_2().text_sm();
    crumbs = crumbs.child(crumb("crumb-root", "Agents", 0, trail.is_empty(), cx));
    for (depth, name) in trail.iter().enumerate() {
        crumbs = crumbs
            .child(div().text_color(muted).child("›"))
            .child(crumb(
                SharedString::from(format!("crumb-{name}")),
                name,
                depth + 1,
                depth + 1 == trail.len(),
                cx,
            ));
    }
    let children = &focus.children;
    let height = (children.len().max(1) as f32) * (CARD_H + GAP);
    let focus_y = height / 2. - CARD_H / 2.;
    let ends: Vec<(f32, f32)> = (0..children.len())
        .map(|index| {
            (
                focus_y + CARD_H / 2.,
                index as f32 * (CARD_H + GAP) + CARD_H / 2.,
            )
        })
        .collect();
    let edges = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let origin = bounds.origin;
            let at = |x: f32, y: f32| point(origin.x + px(x), origin.y + px(y));
            let mut path = PathBuilder::stroke(px(1.5));
            for (from, to) in &ends {
                path.move_to(at(FOCUS_W, *from));
                path.cubic_bezier_to(
                    at(FOCUS_W + BRIDGE, *to),
                    at(FOCUS_W + BRIDGE / 2., *from),
                    at(FOCUS_W + BRIDGE / 2., *to),
                );
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, border);
            }
        },
    )
    .absolute()
    .size_full();
    let focus_card = div()
        .absolute()
        .left_0()
        .top(px(focus_y))
        .w(px(FOCUS_W))
        .h(px(CARD_H))
        .p_3()
        .flex()
        .flex_col()
        .justify_center()
        .gap_1()
        .rounded_lg()
        .border_1()
        .border_color(accent)
        .bg(surface)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(focus.name.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .line_clamp(2)
                .child(if trail.is_empty() {
                    "The chat asks these agents.".to_owned()
                } else {
                    focus.description.clone()
                }),
        );
    let mut picture = div()
        .relative()
        .w(px(FOCUS_W + BRIDGE + CARD_W))
        .h(px(height))
        .child(edges)
        .child(focus_card);
    for (index, child) in children.iter().enumerate() {
        picture = picture.child(
            card(child, &paths, &asked_by, cx)
                .absolute()
                .left(px(FOCUS_W + BRIDGE))
                .top(px(index as f32 * (CARD_H + GAP))),
        );
    }
    let mut page = div()
        .flex()
        .flex_col()
        .gap_4()
        .child(crumbs)
        .child(div().id("agents-level").overflow_x_scroll().child(picture));
    if children.is_empty() {
        page = page.child(widgets::muted("This agent asks no one of its own.", cx).px_0());
    }
    for problem in tree.problems.iter().chain(&tree.skipped) {
        page = page.child(
            div()
                .text_xs()
                .text_color(danger)
                .child(format!("{}: {}", problem.node, problem.reason)),
        );
    }
    page
}

fn card(
    node: &AgentNode,
    paths: &BTreeMap<String, Vec<String>>,
    asked_by: &BTreeMap<String, Vec<String>>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, border, surface, accent) = (
        theme.muted_foreground,
        theme.border,
        theme.muted,
        theme.primary,
    );
    let name = node.name.clone();
    let inside = node.children.len();
    // One line of chips at most, so a card keeps its height; the rest is a count.
    let empty = Vec::new();
    let links: Vec<(&str, &String)> = node
        .links
        .iter()
        .map(|target| ("asks", target))
        .chain(
            asked_by
                .get(&node.name)
                .unwrap_or(&empty)
                .iter()
                .map(|from| ("asked by", from)),
        )
        .collect();
    let mut chips = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .overflow_hidden();
    for (label, target) in links.iter().take(2) {
        let path = paths.get(*target).cloned();
        chips = chips.child(
            div()
                .id(SharedString::from(format!(
                    "chip-{}-{label}-{target}",
                    node.name
                )))
                .flex_none()
                .max_w(px(100.))
                .truncate()
                .px_1p5()
                .rounded_sm()
                .text_xs()
                .text_color(accent)
                .bg(accent.opacity(0.1))
                .cursor_pointer()
                .child(format!("{label} {target}"))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if let Some(path) = &path {
                        // Shows the level the linked agent sits on.
                        let mut parent = path.clone();
                        parent.pop();
                        ws.agent_path = parent;
                        cx.notify();
                    }
                })),
        );
    }
    if links.len() > 2 {
        chips = chips.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(format!("+{}", links.len() - 2)),
        );
    }
    let enter = name.clone();
    let open = name.clone();
    let switch_name = name.clone();
    div()
        .id(SharedString::from(format!("agent-{name}")))
        .w(px(CARD_W))
        .h(px(CARD_H))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .overflow_hidden()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .cursor_pointer()
        .hover(|card| card.border_color(muted.opacity(0.5)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name.clone()),
                )
                .children((inside > 0).then(|| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_0p5()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{inside} inside"))
                        .child(widgets::icon(IconName::ChevronRight, muted))
                }))
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            Switch::new(SharedString::from(format!("agent-switch-{name}")))
                                .checked(node.enabled)
                                .xsmall()
                                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                    ws.switch(CoreSwitch::Agent(switch_name.clone()), *on, cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .truncate()
                .child(node.description.clone()),
        )
        .child(chips)
        // Enters an agent with children of its own; opens the file of one without.
        .on_click(cx.listener(move |ws, _, _, cx| {
            if inside > 0 {
                ws.agent_path.push(enter.clone());
                cx.notify();
            } else {
                ws.open_detail(
                    Detail::Agent {
                        name: open.clone(),
                        file: None,
                    },
                    cx,
                );
            }
        }))
}

fn crumb(
    id: impl Into<ElementId>,
    label: &str,
    depth: usize,
    current: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .cursor_pointer()
        .text_color(if current {
            theme.foreground
        } else {
            theme.muted_foreground
        })
        .child(label.to_owned())
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.agent_path.truncate(depth);
            cx.notify();
        }))
}

/// The node `path` leads to, and the part of the path that exists.
fn focus<'a>(root: &'a AgentNode, path: &[String]) -> (&'a AgentNode, Vec<String>) {
    let mut node = root;
    let mut trail = Vec::new();
    for name in path {
        match node.children.iter().find(|child| &child.name == name) {
            Some(child) => {
                node = child;
                trail.push(name.clone());
            }
            None => break,
        }
    }
    (node, trail)
}

/// Every agent's path from the root, for chips that jump to it.
fn paths(root: &AgentNode) -> BTreeMap<String, Vec<String>> {
    fn walk(node: &AgentNode, trail: &mut Vec<String>, out: &mut BTreeMap<String, Vec<String>>) {
        for child in &node.children {
            trail.push(child.name.clone());
            out.entry(child.name.clone())
                .or_insert_with(|| trail.clone());
            walk(child, trail, out);
            trail.pop();
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// Who links to each agent besides its own parent.
fn asked_by(root: &AgentNode) -> BTreeMap<String, Vec<String>> {
    fn walk(node: &AgentNode, out: &mut BTreeMap<String, Vec<String>>) {
        for link in &node.links {
            out.entry(link.clone()).or_default().push(node.name.clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::{AgentNode, asked_by, focus, paths};

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
    fn levels_are_entered_and_links_read_both_ways() {
        let root = node(
            "chat",
            vec![
                node(
                    "plan",
                    vec![node("a", vec![], &[]), node("b", vec![], &["review"])],
                    &[],
                ),
                node("review", vec![], &[]),
            ],
            &[],
        );
        let (level, trail) = focus(&root, &["plan".into(), "gone".into()]);
        assert_eq!(level.name, "plan");
        assert_eq!(trail, ["plan"]);
        assert_eq!(paths(&root)["b"], ["plan", "b"]);
        assert_eq!(asked_by(&root)["review"], ["b"]);
    }
}
