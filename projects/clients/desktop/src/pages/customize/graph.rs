//! The focused agent and its direct reports. Breadcrumbs keep navigation in the
//! hierarchy; relationship chips link to agents elsewhere in the tree.

use crate::ui::elevation::{Elevation, Level};
use std::collections::BTreeMap;

use dusk_protocol::{
    catalog::{AgentNode, AgentTree, Problem},
    client::Switch as CoreSwitch,
};
use gpui_kit::{
    component::{ActiveTheme, IconName, Sizable, switch::Switch},
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::detail::Detail,
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

const CARD_W: f32 = 288.;
/// A name, two lines of description and a line of chips.
const CARD_H: f32 = 120.;

pub fn render(ws: &Workspace, tree: &AgentTree, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, border, surface) = (theme.muted_foreground, theme.border, theme.muted);
    let paths = paths(&tree.root);
    let asked_by = asked_by(&tree.root);
    let (focus, trail) = focus(&tree.root, &ws.agent_path);
    // Breadcrumb: Agents › a › b, each a way back.
    let mut crumbs = div().flex().flex_row().items_center().gap_2().small();
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
    let summary = div()
        .flex()
        .items_center()
        .flex_wrap()
        .gap_3()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(surface)
        .child(widgets::icon(IconName::Network, muted))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .body()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(focus.name.clone()),
                )
                .when(!trail.is_empty(), |summary| {
                    summary.child(
                        div()
                            .small()
                            .text_color(muted)
                            .line_clamp(2)
                            .child(focus.about()),
                    )
                }),
        )
        .child(
            div()
                .small()
                .text_color(muted)
                .child(widgets::count(children.len(), "direct agent")),
        );
    let mut cards = div().flex().flex_wrap().gap_3();
    for child in children {
        cards = cards.child(card(child, &paths, &asked_by, cx));
    }
    let mut page = div()
        .flex()
        .flex_col()
        .gap_3()
        .when(!trail.is_empty(), |page| page.child(crumbs))
        .child(summary)
        .when(!children.is_empty(), |page| {
            page.child(div().pl_3().border_l_1().border_color(border).child(cards))
        });
    if children.is_empty() {
        page = page.child(widgets::muted("This agent asks no one of its own.", cx).px_0());
    }
    page.children(problems("Could not be read", &tree.problems, cx))
        .children(problems(
            "Links to agents that are not there",
            &tree.skipped,
            cx,
        ))
}

/// What the tree left out, under the graph: a quiet group with one row for each, the
/// agent's name and why. A note to read, not an alarm, so only its mark is colored.
fn problems(title: &str, problems: &[Problem], cx: &App) -> Option<Div> {
    if problems.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let (muted, warning) = (theme.muted_foreground, theme.warning);
    let mut group = div().flex().flex_col().gap_1().pt_3().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .group_name()
            .text_color(muted)
            .child(widgets::icon(IconName::TriangleAlert, warning))
            .child(format!("{title} \u{00B7} {}", problems.len())),
    );
    for problem in problems {
        group = group.child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_3()
                .pl(px(crate::ui::size::ICON + crate::ui::size::GAP_S))
                .small()
                .child(
                    div()
                        .flex_none()
                        .w(px(200.))
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(problem.node.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(muted)
                        .child(problem.reason.clone()),
                ),
        );
    }
    Some(group)
}

fn card(
    node: &AgentNode,
    paths: &BTreeMap<String, Vec<String>>,
    asked_by: &BTreeMap<String, Vec<String>>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.primary);
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
                .focusable_row(cx)
                .flex_none()
                .max_w(px(112.))
                .truncate()
                .px_1p5()
                .rounded_sm()
                .meta()
                .text_color(accent)
                .bg(accent.opacity(0.1))
                .cursor_pointer()
                .child(format!("{label} {target}"))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
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
                .meta()
                .text_color(muted)
                .child(format!("+{}", links.len() - 2)),
        );
    }
    let enter = name.clone();
    let open = name.clone();
    let switch_name = name.clone();
    div()
        .id(SharedString::from(format!("agent-{name}")))
        .focusable_row(cx)
        .w(px(CARD_W))
        .max_w_full()
        .h(px(CARD_H))
        .px_3()
        .py_2p5()
        .flex()
        .flex_col()
        .gap_1()
        .overflow_hidden()
        .rounded_lg()
        .elevation(Level::Raised, cx)
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
                        .meta()
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
                                .small()
                                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                    ws.switch(CoreSwitch::Agent(switch_name.clone()), *on, cx);
                                })),
                        ),
                ),
        )
        // Two lines of what the agent is for; every card is as tall, with or without
        // chips, so a row of them keeps one lower edge.
        .child(
            div()
                .flex_none()
                .meta()
                .line_height(rems(1.2))
                .text_color(muted)
                .line_clamp(2)
                .text_ellipsis()
                .child(node.about()),
        )
        .child(div().flex_1())
        .when(!links.is_empty(), |card| card.child(chips))
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
            source: None,
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
